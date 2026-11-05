// The module's `mln_browser` imports (crates/mln_util/src/browser.rs).
//
// `wait` suspends the calling WebAssembly stack (JSPI) until the browser's
// event loop has run one task. A MessageChannel message is that task: unlike
// setTimeout(0), it is not clamped to 4 ms when the waits nest.

const channel = new MessageChannel();
const waiting = [];
channel.port1.onmessage = () => waiting.shift()?.();

// How often the module waited, for run.mjs's diagnosis of a test that hangs.
export const stats = { waits: 0 };

export const wait = new WebAssembly.Suspending(
  () =>
    new Promise((resolve) => {
      stats.waits += 1;
      waiting.push(resolve);
      channel.port2.postMessage(0);
    }),
);
