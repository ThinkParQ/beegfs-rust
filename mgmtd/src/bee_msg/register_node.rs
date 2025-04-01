use super::*;
use common::update_node;
use shared::bee_msg::node::*;

impl HandleWithResponse for RegisterNode {
    type Response = RegisterNodeResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Result<Self::Response> {
        fail_on_pre_shutdown(ctx)?;

        let node_id = update_node(self, ctx).await?;

        let fs_uuid: String = ctx
            .db
            .read_tx(|tx| db::config::get(tx, db::config::Config::FsUuid))
            .await?
            .ok_or_else(|| anyhow!("Could not read file system UUID from database"))?;

        Ok(RegisterNodeResp {
            node_num_id: node_id,
            grpc_port: ctx.info.user_config.grpc_port,
            fs_uuid: fs_uuid.into_bytes(),
        })
    }
}
