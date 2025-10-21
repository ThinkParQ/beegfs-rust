use super::*;
use common::update_node;
use shared::bee_msg::misc::Ack;
use shared::bee_msg::node::*;

impl HandleWithResponse for Heartbeat {
    type Response = Ack;

    async fn handle(self, app: &impl App, _req: &mut impl Request) -> Result<Self::Response> {
        fail_on_pre_shutdown(app)?;

        update_node(
            RegisterNode {
                instance_version: self.instance_version,
                nic_list_version: self.nic_list_version,
                node_alias: self.node_alias,
                nics: self.nic_list,
                node_type: self.node_type,
                node_id: self.node_num_id,
                root_num_id: self.root_num_id,
                is_root_mirrored: self.is_root_mirrored,
                port: self.port,
                port_tcp_unused: self.port_tcp_unused,
                machine_uuid: self.machine_uuid,
            },
            app,
        )
        .await?;

        Ok(Ack {
            ack_id: self.ack_id,
        })
    }
}
