use super::*;

pub struct MsStoreChannel;

#[async_trait]
impl Channel for MsStoreChannel {
    fn info(&self) -> ChannelInfo {
        ChannelInfo {
            name: "msstore",
            can_activate: false,
            can_self_update: false,
            shows_external_purchase: false,
        }
    }

    fn entitlement(&self) -> Entitlement {
        Entitlement::Unlocked
    }
}
