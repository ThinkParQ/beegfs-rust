use super::*;
use shared::msg::Ack;

pub(super) async fn handle(
    msg: msg::Heartbeat,
    chn: impl RequestChannel,
    hnd: impl ComponentHandles,
) -> Result<()> {
    if let Err(err) = async move {
        hnd.execute_db(move |tx| {
            db::nodes::set(
                tx,
                msg.node_num_id,
                msg.node_type,
                msg.node_alias,
                msg.port,
                msg.nic_list,
            )
        })
        .await?;

        log::info!(
            "Processed {} node heartbeat for ID {}",
            msg.node_type,
            msg.node_num_id,
        );

        Ok(()) as Result<()>
    }
    .await
    {
        log::error!(
            "Processing {} node heartbeat for ID {} failed:\n{:?}",
            msg.node_type,
            msg.node_num_id,
            err
        );
    }

    chn.respond(&Ack { ack_id: msg.ack_id }).await
}
