const fs = require("node:fs");
const path = require("node:path");

const [sourcePath, overlayPath, outputPath] = process.argv.slice(2);
const publicKey = process.env.TAURI_UPDATER_PUBLIC_KEY;

if (!sourcePath || !overlayPath || !outputPath) {
  throw new Error("usage: node prepare-tauri-updater-config.cjs <source> <overlay> <output>");
}
if (!publicKey || publicKey === "REPLACE_WITH_TAURI_UPDATER_PUBLIC_KEY") {
  throw new Error("TAURI_UPDATER_PUBLIC_KEY is required to create updater artifacts");
}

const merge = (base, overlay) => {
  const result = { ...base };
  for (const [key, value] of Object.entries(overlay)) {
    if (value && typeof value === "object" && !Array.isArray(value)) {
      result[key] = merge(result[key] || {}, value);
    } else {
      result[key] = value;
    }
  }
  return result;
};

const config = merge(
  JSON.parse(fs.readFileSync(sourcePath, "utf8")),
  JSON.parse(fs.readFileSync(overlayPath, "utf8")),
);
config.bundle = { ...(config.bundle || {}), createUpdaterArtifacts: true };
config.plugins = {
  ...(config.plugins || {}),
  updater: {
    ...(config.plugins?.updater || {}),
    pubkey: publicKey,
  },
};

fs.mkdirSync(path.dirname(outputPath), { recursive: true });
fs.writeFileSync(outputPath, `${JSON.stringify(config, null, 2)}\n`);
