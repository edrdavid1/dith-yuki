# Store account data

What this repository can prepare, and what only the account holder can supply. Do not commit bank details, tax forms, a home address, or a phone number.

The current agreement names **David Murashka, operating as L'eco non di Bergamo**, under the laws of the Republic of Lithuania. That matches an **individual** Apple Developer enrollment, with the seller name shown as the person. A company seller name is a different enrollment: it needs a legal entity, a D-U-N-S number, and an update to the agreement. Bank and tax forms stay in App Store Connect. They do not belong in git.

## You still have to provide

| Item | Where it goes | Notes |
|---|---|---|
| Apple ID email used to enroll | Apple, not the repo | Individual account under David Murashka |
| Confirmation the listing stays a beta | App Store Connect | The agreement still says the app is beta |
| Team ID (`APPLE_TEAM_ID`) | Shell env when building | 10 letters or digits, after Apple approves the account |
| Mac App Store provisioning profile | `src-tauri/certs/DitherYuki_MAS.provisionprofile` | For `com.dither.app`. Gitignored |
| Apple Distribution certificate | Keychain | Used by `npm run build:mas` |
| Mac Installer Distribution name (`APPLE_INSTALLER_IDENTITY`) | Shell env for `npm run package:mas` | Exact certificate name from Keychain |
| Support URL | App Store Connect | A page you control. The download site or the GitHub repo both qualify once you pick one |
| Privacy policy URL | App Store Connect | `site/privacy.html` once that site is hosted (GitHub Pages and/or ditheryuki.com) |
| Address and phone Apple asks for on the account | Apple only | Do not put them in git |
| Bank account and tax form (W-8BEN for a non-US individual) | App Store Connect | Apple pays the store copy; the agreement does not change that |
| Minimum macOS version on submission day | Decision at upload | The project still declares 10.15. Confirm it against the SDK Apple requires that day |
| Microsoft Partner Center publisher id | Later | Not part of this Mac App Store release |

## What the repo does with that

`npm run build:mas` refuses to start until `APPLE_TEAM_ID` is set. It writes `src-tauri/entitlements/mas.generated.entitlements` from `src-tauri/entitlements/mas.entitlements`, with the team id filled in, and it requires the provisioning profile above. Hardened runtime is turned on for that build only.

`npm run package:mas` wraps the universal `.app` with `productbuild` and `APPLE_INSTALLER_IDENTITY`. Upload that pkg with Apple's Transporter. The script does not upload it.

Direct and Microsoft Store builds are unchanged: `npm run build:direct` and `npm run build:msstore`. Haptics stay on those two. The Mac App Store build leaves them out. Store builds hide Help → Report a bug and do not open remote DevTools.
