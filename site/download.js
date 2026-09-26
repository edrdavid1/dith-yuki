const RELEASES_API =
  "https://api.github.com/repos/edrdavid1/dith-yuki/releases/latest";
const RELEASES_FALLBACK =
  "https://github.com/edrdavid1/dith-yuki/releases/latest";

function formatBytes(bytes) {
  if (!Number.isFinite(bytes) || bytes <= 0) return "";
  const mb = bytes / (1024 * 1024);
  return `${mb.toFixed(mb >= 10 ? 0 : 1)} MB`;
}

function findAsset(assets, predicate) {
  return assets.find(predicate) ?? null;
}

function wireButton(el, asset, label) {
  if (!el) return;
  if (!asset?.browser_download_url) {
    el.href = RELEASES_FALLBACK;
    el.removeAttribute("download");
    return;
  }

  el.href = asset.browser_download_url;
  el.setAttribute("download", asset.name);
  const size = formatBytes(asset.size);
  el.title = size ? `${label} · ${size}` : label;
  el.removeAttribute("aria-disabled");
  el.classList.remove("is-loading");
}

async function wireDownloads() {
  const macBtn = document.getElementById("dl-mac");
  const winBtn = document.getElementById("dl-win");
  const versionEl = document.getElementById("version");

  [macBtn, winBtn].forEach((btn) => {
    if (!btn) return;
    btn.classList.add("is-loading");
    btn.setAttribute("aria-disabled", "true");
  });

  try {
    const res = await fetch(RELEASES_API, {
      headers: { Accept: "application/vnd.github+json" },
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);

    const release = await res.json();
    const assets = Array.isArray(release.assets) ? release.assets : [];

    const mac = findAsset(
      assets,
      (a) => typeof a.name === "string" && a.name.endsWith(".dmg"),
    );
    const win = findAsset(
      assets,
      (a) =>
        typeof a.name === "string" &&
        a.name.endsWith("-setup.exe") &&
        !a.name.endsWith(".sig"),
    );

    wireButton(macBtn, mac, "macOS Apple Silicon DMG");
    wireButton(winBtn, win, "Windows x64 installer");

    if (versionEl && release.tag_name) {
      versionEl.textContent = release.tag_name;
    }
  } catch {
    wireButton(macBtn, null, "macOS");
    wireButton(winBtn, null, "Windows");
    if (versionEl) versionEl.textContent = "latest";
  }
}

wireDownloads();
