import { invoke } from '@tauri-apps/api/core';

export type DistributionChannel = 'direct' | 'mas' | 'msstore';

export type Entitlement =
  | { state: 'unlocked' }
  | { state: 'locked'; reason: 'no_license' | 'expired' | 'wrong_device' | 'invalid' };

export type ChannelInfo = {
  name: DistributionChannel;
  canActivate: boolean;
  canSelfUpdate: boolean;
  showsExternalPurchase: boolean;
};

export function fetchChannelInfo(): Promise<ChannelInfo> {
  return invoke<ChannelInfo>('channel_info');
}

export function fetchEntitlement(): Promise<Entitlement> {
  return invoke<Entitlement>('entitlement');
}

export function activateLicense(key: string): Promise<Entitlement> {
  return invoke<Entitlement>('activate_license', { key });
}

export function deactivateLicense(): Promise<void> {
  return invoke('deactivate_license');
}

export function refreshLicense(): Promise<Entitlement> {
  return invoke<Entitlement>('refresh_license');
}
