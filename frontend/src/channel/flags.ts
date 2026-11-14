/** Build channel only. No other channel imports, so this cannot cycle. */
declare const __CHANNEL__: 'direct' | 'mas' | 'msstore';

export const CHANNEL = __CHANNEL__;
export const IS_DIRECT = __CHANNEL__ === 'direct';
