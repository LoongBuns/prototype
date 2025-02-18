import { pipeline } from "node:stream/promises";
import zlib from "node:zlib";
import path from "node:path";
import fs from "node:fs";

const DOWNLOAD_URL = "https://github.com/bytecodealliance/javy/releases/download/v5.0.1/javy-x86_64-linux-v5.0.1.gz";
const JAVY_BIN = path.join(import.meta.dirname, "../node_modules/.bin", "javy");
const TEMP = path.join(import.meta.dirname, "javy.gz");

if (fs.existsSync(JAVY_BIN)) {
    process.exit(0);
}

async function downloadJavy() {
    try {
        const response = await fetch(DOWNLOAD_URL);
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
