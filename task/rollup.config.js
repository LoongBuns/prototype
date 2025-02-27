import resolve from "@rollup/plugin-node-resolve";

export default {
    input: [
        "src/fiber.js",
        "src/fractal.js",
        "src/render.js",
    ],
    output: {
        dir: "dist",
        format: "esm",
    },
    plugins: [
        resolve(),
    ]
}
