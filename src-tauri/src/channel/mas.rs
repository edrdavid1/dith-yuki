use super::*;

pub struct MasChannel;

#[async_trait]
impl Channel for MasChannel {
    fn info(&self) -> ChannelInfo {
        ChannelInfo {
            name: "mas",
            can_activate: false,
            can_self_update: false,
            shows_external_purchase: false,
        }
    }

    fn entitlement(&self) -> Entitlement {
        Entitlement::Unlocked
    }
}
