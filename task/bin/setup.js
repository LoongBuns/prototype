import { pipeline } from "node:stream/promises";
import zlib from "node:zlib";
import path from "node:path";
import fs from "node:fs";
import os from "node:os";

const VERSION = "v5.0.1";
const PACKAGE = {
    "darwin": {
        "x64": `javy-x86_64-macos-${VERSION}.gz`,
        "arm": `javy-arm-macos-${VERSION}.gz`
    },
    "linux": {
        "x64": `javy-x86_64-linux-${VERSION}.gz`,
        "arm": `javy-arm-linux-${VERSION}.gz`
    },
    "win32": `javy-x86_64-windows-${VERSION}.gz`
};
const DOWNLOAD_URL = `https://github.com/bytecodealliance/javy/releases/download/${VERSION}`;
const JAVY_BIN = path.join(import.meta.dirname, "../node_modules/.bin", os.platform() === "win32" ? "javy.exe" : "javy");
const TEMP = path.join(import.meta.dirname, "javy.gz");

if (fs.existsSync(JAVY_BIN)) {
    process.exit(0);
}

async function downloadJavy() {
    try {
        let platform = os.platform();
        if (platform !== "win32" && platform !== "darwin")
            platform = "linux";

        let arch = os.arch();
        arch = (arch === "arm" || arch === "arm64") ? "arm" : "x64";

        const url = `${DOWNLOAD_URL}/${PACKAGE[platform]?.[arch] || PACKAGE[platform] || ""}`;
        const response = await fetch(url);
        if (!response.ok) {
            throw new Error(`Failed to download: ${response.status} ${response.statusText}`);
        }

        const fileStream = fs.createWriteStream(TEMP);
        await pipeline(response.body, fileStream);

        const gunzip = zlib.createGunzip();
        const input = fs.createReadStream(TEMP);
        const output = fs.createWriteStream(JAVY_BIN);
        await pipeline(input, gunzip, output);

        fs.chmodSync(JAVY_BIN, 0o755);
        fs.unlinkSync(TEMP);
    } catch (error) {
        if (fs.existsSync(TEMP)) {
            fs.unlinkSync(TEMP);
        }

        process.exit(1);
    }
}

downloadJavy().catch(error => {
    console.error("Failed to install Javy:", error);
    process.exit(1);
});
