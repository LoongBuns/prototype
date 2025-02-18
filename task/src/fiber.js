import {use, renderSync, useState} from "@use-gpu/live";

function assertEqual(actual, expected, message) {
    if (actual !== expected) {
        throw new Error(message || `Expected ${expected}, but got ${actual}`);
    }
}

function assertDeepEqual(actual, expected, message) {
    const a = JSON.stringify(actual);
    const e = JSON.stringify(expected);
    if (a !== e) {
        throw new Error(message || `Expected ${e}, but got ${a}`);
    }
}

export function run() {
    const rendered = {
        root: 0,
        node: 0,
    };
    let trigger = null;
    const setTrigger = (f) => trigger = f;

    const Root = () => {
        const [value, setValue] = useState(0);
        setTrigger(() => setValue(1));

        rendered.root++;
        return [
            use(Node),
            null,
            value ? null : use(Node),
            value ? use(Node) : null,
            use(Node),
        ];
    };

    const Node = () => {
        rendered.node++;
    }

    const result = renderSync(use(Root));
    if (!result.host) return;

    const {host: {flush, __stats: stats}} = result;

    assertEqual(result.f, Root, "result.f should be Root");
    if (result.mounts) {
        assertDeepEqual(result.order, [0, 2, 4], "Initial order should be [0, 2, 4]");
    }

    assertEqual(rendered.root, 1, "rendered.root should be 1");
    assertEqual(rendered.node, 3, "rendered.node should be 3");

    assertEqual(stats.mounts, 4, "stats.mounts should be 4");
    assertEqual(stats.unmounts, 0, "stats.unmounts should be 0");
    assertEqual(stats.updates, 0, "stats.updates should be 0");
    assertEqual(stats.dispatch, 1, "stats.dispatch should be 1");

    if (trigger) trigger();
    if (flush) flush();

    assertEqual(result.f, Root, "result.f should still be Root after update");
    if (result.mounts) {
        assertDeepEqual(result.order, [0, 3, 4], "After update, order should be [0, 3, 4]");
    }

    assertEqual(rendered.root, 2, "rendered.root should be 2 after update");
    assertEqual(rendered.node, 6, "rendered.node should be 6 after update");

    assertEqual(stats.mounts, 5, "stats.mounts should be 5 after update");
    assertEqual(stats.unmounts, 1, "stats.unmounts should be 1 after update");
    assertEqual(stats.updates, 2, "stats.updates should be 2 after update");
    assertEqual(stats.dispatch, 2, "stats.dispatch should be 2 after update");
}
