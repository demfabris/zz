#!/usr/bin/env node
import { spawn } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const USAGE = `usage: scripts/storybook-shot.mjs <story[/section][?knobs]> <out.png> [options]

Renders one storybook story in headless Chrome and saves a PNG. With a section,
the image is clipped to that section; otherwise it is the whole page.

options:
  --url <base>        storybook address (default http://127.0.0.1:8097/)
  --width <px>        viewport width (default 1400)
  --height <px>       viewport height (default 1000)
  --scale <n>         device pixel ratio (default 2)
  --timeout <ms>      give up after this long (default 60000)

environment: CHROME overrides the browser binary.`;

function parseArgs(argv) {
    const options = { url: "http://127.0.0.1:8097/", width: 1400, height: 1000, scale: 2, timeout: 60000 };
    const positional = [];
    for (let i = 0; i < argv.length; i++) {
        const arg = argv[i];
        if (arg === "-h" || arg === "--help") {
            console.log(USAGE);
            process.exit(0);
        }
        if (arg.startsWith("--")) {
            const key = arg.slice(2);
            if (!(key in options)) throw new Error(`unknown option ${arg}`);
            const value = argv[++i];
            options[key] = typeof options[key] === "number" ? Number(value) : value;
        } else {
            positional.push(arg);
        }
    }
    if (positional.length !== 2) throw new Error(USAGE);
    return { route: positional[0].replace(/^#?\/?/, ""), out: positional[1], ...options };
}

function chromeBinary() {
    if (process.env.CHROME) return process.env.CHROME;
    if (process.platform === "darwin") return "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
    return "google-chrome";
}

function launch(width, height, scale) {
    const profile = mkdtempSync(join(tmpdir(), "zz-storybook-shot-"));
    const args = [
        "--headless=new",
        "--remote-debugging-port=0",
        `--user-data-dir=${profile}`,
        "--enable-unsafe-webgpu",
        "--hide-scrollbars",
        "--no-first-run",
        "--no-default-browser-check",
        `--window-size=${width},${height}`,
        `--force-device-scale-factor=${scale}`,
        "about:blank",
    ];
    if (process.platform === "darwin") args.splice(4, 0, "--use-angle=metal");
    if (process.platform === "linux") args.splice(4, 0, "--enable-features=Vulkan", "--use-angle=vulkan");
    const child = spawn(chromeBinary(), args, { stdio: ["ignore", "ignore", "pipe"] });
    const endpoint = new Promise((resolve, reject) => {
        let buffer = "";
        child.stderr.on("data", (chunk) => {
            buffer += chunk;
            const match = buffer.match(/DevTools listening on (ws:\/\/\S+)/);
            if (match) resolve(match[1]);
        });
        child.on("exit", (code) => reject(new Error(`chrome exited with ${code} before DevTools started`)));
    });
    const stop = () => {
        child.kill("SIGKILL");
        try {
            rmSync(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
        } catch {}
    };
    return { endpoint, stop };
}

function connect(url) {
    const socket = new WebSocket(url);
    let next = 1;
    const pending = new Map();
    const listeners = [];
    socket.addEventListener("message", (event) => {
        const message = JSON.parse(event.data);
        if (message.id && pending.has(message.id)) {
            const { resolve, reject } = pending.get(message.id);
            pending.delete(message.id);
            if (message.error) reject(new Error(message.error.message));
            else resolve(message.result);
        } else if (message.method) {
            for (const listener of listeners) listener(message);
        }
    });
    const opened = new Promise((resolve, reject) => {
        socket.addEventListener("open", resolve, { once: true });
        socket.addEventListener("error", reject, { once: true });
    });
    const send = (method, params = {}, sessionId) =>
        new Promise((resolve, reject) => {
            const id = next++;
            pending.set(id, { resolve, reject });
            socket.send(JSON.stringify({ id, method, params, sessionId }));
        });
    return { opened, send, on: (listener) => listeners.push(listener), close: () => socket.close() };
}

async function main() {
    const options = parseArgs(process.argv.slice(2));
    const [path] = options.route.split("?");
    const [story, section] = path.split("/");
    const chrome = launch(options.width, options.height, options.scale);
    const deadline = setTimeout(() => {
        console.error(`timed out after ${options.timeout} ms`);
        chrome.stop();
        process.exit(1);
    }, options.timeout);
    try {
        const cdp = connect(await chrome.endpoint);
        await cdp.opened;
        const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
        const { sessionId } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
        const send = (method, params) => cdp.send(method, params, sessionId);
        const problems = [];
        cdp.on((message) => {
            if (message.sessionId !== sessionId) return;
            if (message.method === "Runtime.exceptionThrown") {
                problems.push(message.params.exceptionDetails.exception?.description ?? message.params.exceptionDetails.text);
            } else if (message.method === "Runtime.consoleAPICalled" && message.params.type === "error") {
                problems.push(message.params.args.map((arg) => arg.value ?? arg.description).join(" "));
            }
        });
        await send("Runtime.enable");
        await send("Page.enable");
        const url = new URL(options.url);
        url.hash = `#/${options.route}`;
        await send("Page.navigate", { url: url.toString() });
        const ready = await send("Runtime.evaluate", {
            awaitPromise: true,
            returnByValue: true,
            expression: `(async () => {
                while (!window.storybook || window.storybook.current() !== ${JSON.stringify(story)}) {
                    await new Promise((resolve) => setTimeout(resolve, 50));
                }
                for (const element of [document.body, document.getElementById("story")]) {
                    element.style.height = "auto";
                    element.style.overflow = "visible";
                }
                await zzGpui.idle();
                await zzGpui.idle();
                const section = ${JSON.stringify(section ?? null)};
                const target = section && document.getElementById(${JSON.stringify(story)} + "-" + section);
                if (section && !target) throw new Error("no section " + section);
                const rect = target?.getBoundingClientRect();
                return {
                    windows: zzGpui.windows().map((window) => ({ id: window.id, mount: window.mount, height: window.height })),
                    clip: rect ? { x: rect.left + scrollX - 16, y: rect.top + scrollY - 16, width: rect.width + 32, height: rect.height + 32 } : null,
                };
            })()`,
        });
        if (ready.exceptionDetails) throw new Error(ready.exceptionDetails.exception?.description ?? "page failed");
        const { windows, clip } = ready.result.value;
        const shot = await send("Page.captureScreenshot", {
            format: "png",
            captureBeyondViewport: true,
            ...(clip ? { clip: { ...clip, scale: 1 } } : {}),
        });
        writeFileSync(options.out, Buffer.from(shot.data, "base64"));
        console.log(JSON.stringify({ out: options.out, windows, problems }, null, 2));
        cdp.close();
    } finally {
        clearTimeout(deadline);
        chrome.stop();
    }
}

main().catch((error) => {
    console.error(error.message);
    process.exit(1);
});
