import {
  activateLicense,
  deactivateLicense,
  fetchChannelInfo,
  fetchEntitlement,
  refreshLicense,
  type ChannelInfo,
  type Entitlement,
} from '../shared/ipc/channel';
import { CHANNEL, IS_DIRECT } from './flags';

export { ChannelGates } from './ChannelGates';
export type { ChannelInfo, Entitlement };
export { CHANNEL, IS_DIRECT };

export async function bootChannel(): Promise<ChannelInfo> {
  const info = await fetchChannelInfo();
  if (info.name !== CHANNEL) {
    throw new Error(`Channel mismatch: rust=${info.name}, web=${CHANNEL}`);
  }
  return info;
}

export { activateLicense, deactivateLicense, refreshLicense };
export const getEntitlement = fetchEntitlement;
