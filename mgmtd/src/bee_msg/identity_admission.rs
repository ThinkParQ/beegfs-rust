use super::*;
use crate::config::{Config, RegisterClientMode, RegisterServerMode, TokenScope};
use aws_lc_rs::constant_time::verify_slices_are_equal;
use aws_lc_rs::digest::{SHA256, digest};
use aws_lc_rs::rand::{SecureRandom, SystemRandom};
use rusqlite::{OptionalExtension, Transaction, params};
use shared::conn::IdentityId;
use shared::conn::beecrypt::KeyPairId;

const TOKEN_PREFIX: &str = "beegfs-idt-1";
const TOKEN_SECRET_LEN: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyCategory {
    Client,
    Server,
}

impl KeyCategory {
    pub(crate) fn of_node_type(node_type: NodeType) -> Self {
        match node_type {
            NodeType::Client => Self::Client,
            _ => Self::Server,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::Server => "server",
        }
    }
}

fn scope_str(scope: TokenScope) -> &'static str {
    match scope {
        TokenScope::Client => "client",
        TokenScope::Admin => "admin",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AdmissionModes {
    pub client: RegisterClientMode,
    pub server: RegisterServerMode,
}

impl AdmissionModes {
    pub(crate) fn from_config(config: &Config) -> Self {
        Self {
            client: config.register_client_mode,
            server: config.register_server_mode,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct KeyRequest {
    pub key_id: KeyPairId,
    pub category: KeyCategory,
    pub token: Option<String>,
    pub node: Option<(NodeType, NodeId)>,
    pub name: Option<String>,
    pub addr: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Admission {
    Registered(IdentityId),
    Pending,
    Rejected(String),
}

pub(crate) fn admit_key(
    tx: &Transaction,
    modes: AdmissionModes,
    req: &KeyRequest,
) -> Result<Admission> {
    let key = req.key_id.to_string();

    let known: Option<IdentityId> = tx
        .query_row_cached(
            sql!("SELECT identity_id FROM keys WHERE key = ?1"),
            [&key],
            |row| row.get(0),
        )
        .optional()?;

    if let Some(identity_id) = known {
        return Ok(Admission::Registered(identity_id));
    }

    let name = req.node.is_none().then_some(req.name.as_deref()).flatten();

    if let Some(token) = &req.token {
        if req.category == KeyCategory::Server {
            return Ok(Admission::Rejected(
                "Identity tokens are only accepted for clients".to_string(),
            ));
        }

        return Ok(match use_token(tx, token)? {
            Ok((token_id, scope)) => Admission::Registered(insert_identity(
                tx,
                &key,
                name,
                scope == TokenScope::Admin,
                Some(token_id),
            )?),
            Err(reason) => Admission::Rejected(reason),
        });
    }

    let (automatic, approval) = match req.category {
        KeyCategory::Client => match modes.client {
            RegisterClientMode::Automatic => (true, false),
            RegisterClientMode::Token => (false, false),
            RegisterClientMode::Approval | RegisterClientMode::Both => (false, true),
        },
        KeyCategory::Server => match modes.server {
            RegisterServerMode::Automatic => (true, false),
            RegisterServerMode::Approval => (false, true),
            RegisterServerMode::Disabled => (false, false),
        },
    };

    if automatic {
        return Ok(Admission::Registered(insert_identity(
            tx, &key, name, false, None,
        )?));
    }

    if !approval {
        return Ok(Admission::Rejected(match req.category {
            KeyCategory::Client => "New client identities require an identity token".to_string(),
            KeyCategory::Server => "Registration of new server identities is disabled".to_string(),
        }));
    }

    let rejected: Option<bool> = tx
        .query_row_cached(
            sql!("SELECT rejected FROM pending_keys WHERE key = ?1"),
            [&key],
            |row| row.get(0),
        )
        .optional()?;

    if rejected == Some(true) {
        return Ok(Admission::Rejected(format!(
            "Key pair ID {key} has been rejected by an admin"
        )));
    }

    let (node_type, node_id) = req.node.unzip();

    tx.execute_cached(
        sql!(
            "INSERT INTO pending_keys (key, category, node_type, node_id, name, addr)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT (key) DO UPDATE SET
                category = excluded.category,
                node_type = excluded.node_type,
                node_id = excluded.node_id,
                name = excluded.name,
                addr = excluded.addr,
                last_seen = DATETIME('now')"
        ),
        params![
            key,
            req.category.as_str(),
            node_type.map(|t| t.sql_variant()),
            node_id,
            name,
            req.addr
        ],
    )?;

    Ok(Admission::Pending)
}

pub fn create_identity_token(
    tx: &Transaction,
    scope: TokenScope,
    permanent: bool,
    comment: Option<&str>,
) -> Result<String> {
    let mut secret = [0u8; TOKEN_SECRET_LEN];
    SystemRandom::new()
        .fill(&mut secret)
        .map_err(|_| anyhow!("Generating a random identity token failed"))?;
    let secret = to_hex(&secret);

    tx.execute_cached(
        sql!(
            "INSERT INTO identity_tokens (secret_hash, scope, permanent, comment)
            VALUES (?1, ?2, ?3, ?4)"
        ),
        params![hash_secret(&secret), scope_str(scope), permanent, comment],
    )?;

    Ok(format!(
        "{TOKEN_PREFIX}.{}.{secret}",
        tx.last_insert_rowid()
    ))
}

pub(crate) fn approve_pending_key(tx: &Transaction, key_id: &KeyPairId) -> Result<IdentityId> {
    let key = key_id.to_string();

    let pending: Option<(Option<NodeType>, Option<String>)> = tx
        .query_row_cached(
            sql!("SELECT node_type, name FROM pending_keys WHERE key = ?1"),
            [&key],
            |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?
                        .map(NodeType::from_sql_variant)
                        .transpose()?,
                    row.get(1)?,
                ))
            },
        )
        .optional()?;

    let Some((node_type, name)) = pending else {
        bail!("No pending entry for key pair ID {key}");
    };

    tx.execute_cached(sql!("DELETE FROM pending_keys WHERE key = ?1"), [&key])?;

    let name = node_type.is_none().then_some(name).flatten();
    insert_identity(tx, &key, name.as_deref(), false, None)
}

pub(crate) fn reject_pending_key(tx: &Transaction, key_id: &KeyPairId) -> Result<()> {
    let affected = tx.execute_cached(
        sql!("UPDATE pending_keys SET rejected = 1 WHERE key = ?1"),
        [key_id.to_string()],
    )?;

    if affected == 0 {
        bail!("No pending entry for key pair ID {key_id}");
    }

    Ok(())
}

fn insert_identity(
    tx: &Transaction,
    key: &str,
    name: Option<&str>,
    admin: bool,
    token_id: Option<i64>,
) -> Result<IdentityId> {
    tx.execute_cached(
        sql!("INSERT INTO identities (name, admin, token_id) VALUES (?1, ?2, ?3)"),
        params![name, admin, token_id],
    )?;
    let identity_id = tx.last_insert_rowid();

    tx.execute_cached(
        sql!("INSERT INTO keys (key, identity_id) VALUES (?1, ?2)"),
        params![key, identity_id],
    )?;

    Ok(identity_id)
}

fn use_token(
    tx: &Transaction,
    token: &str,
) -> Result<std::result::Result<(i64, TokenScope), String>> {
    let parsed = token
        .trim()
        .strip_prefix(TOKEN_PREFIX)
        .and_then(|rest| rest.strip_prefix('.'))
        .and_then(|rest| rest.split_once('.'))
        .and_then(|(id, secret)| Some((id.parse::<i64>().ok()?, secret)));

    let Some((token_id, secret)) = parsed else {
        return Ok(Err("Malformed identity token".to_string()));
    };

    let row: Option<(String, String, bool, Option<String>)> = tx
        .query_row_cached(
            sql!(
                "SELECT secret_hash, scope, permanent, used_at FROM identity_tokens
                WHERE token_id = ?1"
            ),
            [token_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;

    let Some((secret_hash, scope, permanent, used_at)) = row else {
        return Ok(Err("Invalid identity token".to_string()));
    };

    if verify_slices_are_equal(hash_secret(secret).as_bytes(), secret_hash.as_bytes()).is_err() {
        return Ok(Err("Invalid identity token".to_string()));
    }

    if !permanent && used_at.is_some() {
        return Ok(Err(format!(
            "Identity token {token_id} has already been used"
        )));
    }

    let scope = match scope.as_str() {
        "client" => TokenScope::Client,
        "admin" => TokenScope::Admin,
        s => bail!("Identity token {token_id} has an unknown scope {s:?}"),
    };

    tx.execute_cached(
        sql!("UPDATE identity_tokens SET used_at = DATETIME('now') WHERE token_id = ?1"),
        [token_id],
    )?;

    Ok(Ok((token_id, scope)))
}

fn hash_secret(secret: &str) -> String {
    to_hex(digest(&SHA256, secret.as_bytes()).as_ref())
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::db::test::with_test_data;

    fn key(c: &str) -> KeyPairId {
        format!("sha256:{}", c.repeat(64)).parse().unwrap()
    }

    fn client(c: &str, token: Option<&str>) -> KeyRequest {
        KeyRequest {
            key_id: key(c),
            category: KeyCategory::Client,
            token: token.map(str::to_string),
            node: None,
            name: Some(format!("ctl-{c}")),
            addr: "10.0.0.1:1234".to_string(),
        }
    }

    fn server(c: &str) -> KeyRequest {
        KeyRequest {
            key_id: key(c),
            category: KeyCategory::Server,
            token: None,
            node: Some((NodeType::Storage, 0)),
            name: None,
            addr: "10.0.0.2:8003".to_string(),
        }
    }

    fn modes(client: RegisterClientMode, server: RegisterServerMode) -> AdmissionModes {
        AdmissionModes { client, server }
    }

    fn is_admin(tx: &Transaction, identity_id: IdentityId) -> bool {
        tx.query_row(
            "SELECT admin FROM identities WHERE identity_id = ?1",
            [identity_id],
            |row| row.get(0),
        )
        .unwrap()
    }

    fn pending_count(tx: &Transaction) -> i64 {
        tx.query_row("SELECT COUNT(*) FROM pending_keys", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn automatic() {
        with_test_data(|tx| {
            let m = modes(RegisterClientMode::Automatic, RegisterServerMode::Automatic);

            let Admission::Registered(id) = admit_key(tx, m, &client("a", None)).unwrap() else {
                panic!("not registered");
            };
            assert!(!is_admin(tx, id));
            assert_eq!(
                admit_key(tx, m, &client("a", Some("garbage"))).unwrap(),
                Admission::Registered(id)
            );

            assert!(matches!(
                admit_key(tx, m, &server("b")).unwrap(),
                Admission::Registered(_)
            ));
            assert!(matches!(
                admit_key(tx, m, &client("c", Some("garbage"))).unwrap(),
                Admission::Rejected(_)
            ));
        });
    }

    #[test]
    fn tokens() {
        with_test_data(|tx| {
            let m = modes(RegisterClientMode::Token, RegisterServerMode::Automatic);

            assert!(matches!(
                admit_key(tx, m, &client("a", None)).unwrap(),
                Admission::Rejected(_)
            ));

            let once = create_identity_token(tx, TokenScope::Client, false, None).unwrap();
            assert!(matches!(
                admit_key(tx, m, &client("a", Some(&once))).unwrap(),
                Admission::Registered(_)
            ));
            assert!(matches!(
                admit_key(tx, m, &client("b", Some(&once))).unwrap(),
                Admission::Rejected(_)
            ));

            let permanent =
                create_identity_token(tx, TokenScope::Client, true, Some("lab")).unwrap();
            for c in ["c", "d"] {
                assert!(matches!(
                    admit_key(tx, m, &client(c, Some(&permanent))).unwrap(),
                    Admission::Registered(_)
                ));
            }

            let admin = create_identity_token(tx, TokenScope::Admin, false, None).unwrap();
            let Admission::Registered(id) = admit_key(tx, m, &client("e", Some(&admin))).unwrap()
            else {
                panic!("not registered");
            };
            assert!(is_admin(tx, id));

            let (prefix, _) = permanent.rsplit_once('.').unwrap();
            let forged = format!("{prefix}.{}", "0".repeat(64));
            for bad in [forged.as_str(), "beegfs-idt-1.x.y", "nonsense"] {
                assert!(matches!(
                    admit_key(tx, m, &client("f", Some(bad))).unwrap(),
                    Admission::Rejected(_)
                ));
            }

            let mut server_with_token = server("9");
            server_with_token.token = Some(permanent);
            assert!(matches!(
                admit_key(tx, m, &server_with_token).unwrap(),
                Admission::Rejected(_)
            ));
        });
    }

    #[test]
    fn approval() {
        with_test_data(|tx| {
            let m = modes(RegisterClientMode::Both, RegisterServerMode::Approval);

            assert_eq!(admit_key(tx, m, &server("a")).unwrap(), Admission::Pending);
            assert_eq!(admit_key(tx, m, &server("a")).unwrap(), Admission::Pending);
            assert_eq!(
                admit_key(tx, m, &client("b", None)).unwrap(),
                Admission::Pending
            );
            assert_eq!(pending_count(tx), 2);

            let token = create_identity_token(tx, TokenScope::Client, false, None).unwrap();
            assert!(matches!(
                admit_key(tx, m, &client("c", Some(&token))).unwrap(),
                Admission::Registered(_)
            ));

            let id = approve_pending_key(tx, &key("a")).unwrap();
            assert_eq!(
                admit_key(tx, m, &server("a")).unwrap(),
                Admission::Registered(id)
            );

            reject_pending_key(tx, &key("b")).unwrap();
            assert!(matches!(
                admit_key(tx, m, &client("b", None)).unwrap(),
                Admission::Rejected(_)
            ));
            assert_eq!(pending_count(tx), 1);

            approve_pending_key(tx, &key("8")).unwrap_err();
            reject_pending_key(tx, &key("8")).unwrap_err();
        });
    }

    #[test]
    fn disabled_servers() {
        with_test_data(|tx| {
            let m = modes(RegisterClientMode::Approval, RegisterServerMode::Disabled);

            assert!(matches!(
                admit_key(tx, m, &server("a")).unwrap(),
                Admission::Rejected(_)
            ));
            assert_eq!(pending_count(tx), 0);
        });
    }
}
