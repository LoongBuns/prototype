import { execSync } from "node:child_process";
import path from "node:path";
import fs from "node:fs";

const DIST_DIR = path.join(import.meta.dirname, "../dist");
const JAVY_BIN = path.join(import.meta.dirname, "../node_modules/.bin", "javy");

function build() {
    try {
        console.log("\n[1/2] Building with Rollup...");

        execSync("rollup -c", { stdio: "inherit" });

        console.log("\n[2/2] Generating WebAssembly files:");

        const jsFiles = fs.readdirSync(DIST_DIR)
            .filter(file => file.endsWith(".js"))
            .map(file => path.join(DIST_DIR, file));

        if (jsFiles.length === 0) {
            throw new Error("No JS files found in dist directory");
        }

        jsFiles.forEach((jsPath, index) => {
            const wasmName = `${path.basename(jsPath, ".js")}.wasm`;
            const wasmPath = path.join(DIST_DIR, wasmName);

            console.log(`  (${index + 1}/${jsFiles.length}) Converting ${path.basename(jsPath)}`);

            execSync(`${JAVY_BIN} build ${jsPath} -o ${wasmPath}`, { stdio: "inherit" });
        })

        console.log("\n✅ Build completed!");
        console.log(`   Generated ${jsFiles.length} WASM files in ${DIST_DIR}`);
    } catch (error) {
        console.error("\n❌ Build failed:");
        console.error(error.message);
        process.exit(1);
    }
}

build();
