use super::*;
use db::node_nic::map_bee_msg_nics;
use shared::bee_msg::node::*;

impl HandleWithResponse for HeartbeatRequest {
    type Response = Heartbeat;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Result<Self::Response> {
        let (alias, nics) = ctx
            .db
            .read_tx(|tx| {
                Ok((
                    db::entity::get_alias(tx, MGMTD_UID)?
                        .ok_or_else(|| TypedError::value_not_found("management uid", MGMTD_UID))?,
                    db::node_nic::get_with_node(tx, MGMTD_UID)?,
                ))
            })
            .await
            .unwrap_or_default();

        let (alias, nics) = (alias, map_bee_msg_nics(nics).collect());

        let resp = Heartbeat {
            instance_version: 0,
            nic_list_version: 0,
            node_type: shared::types::NodeType::Management,
            node_alias: alias.into_bytes(),
            ack_id: "".into(),
            node_num_id: MGMTD_ID,
            root_num_id: 0,
            is_root_mirrored: 0,
            port: ctx.info.user_config.beemsg_port,
            port_tcp_unused: ctx.info.user_config.beemsg_port,
            nic_list: nics,
            machine_uuid: vec![], // No need for the other nodes to know machine UUIDs
        };

        Ok(resp)
    }
}
