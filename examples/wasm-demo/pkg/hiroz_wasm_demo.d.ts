declare namespace wasm_bindgen {
    /* tslint:disable */
    /* eslint-disable */

    /**
     * Initialize the threaded WASM runtime.
     *
     * Creates Web Workers for each ZRuntime variant, sharing the WASM module and
     * linear memory via SharedArrayBuffer.
     *
     * # Arguments
     * * `shim_url` — URL to the wasm-bindgen JS shim file (e.g., `"./pkg/my_crate.js"`).
     *   Workers will load this via `importScripts()`.
     *
     * # Returns
     * `true` if threaded mode was activated, `false` if falling back to single-threaded
     * (e.g., SharedArrayBuffer not available due to missing COOP/COEP headers).
     */
    export function __zenoh_init_threaded_runtime(shim_url: string): boolean;

    /**
     * Entry point called by each Web Worker after WASM module is initialized
     * with shared memory.
     *
     * Compute workers (Application, TX, RX, Net) run a pure-Rust [`LocalExecutor`]
     * that blocks the worker thread forever. Their futures are woken via
     * `Condvar::notify` (`memory.atomic.notify`), which works from any thread.
     * No JS is used on these workers after entry — `setTimeout`/`spawn_local`
     * would never fire since the event loop is permanently blocked.
     *
     * The Acceptor (I/O worker) keeps its JS event loop alive for WebSocket
     * callbacks. Its task drain uses setTimeout-based self-repolling, since
     * JS microtask wakers cannot be triggered reliably from other threads.
     */
    export function __zenoh_worker_entry(variant_id: number): void;

    /**
     * Spawn the ROS worker task on the Application worker. Call once, after
     * `ros_start` returned true.
     */
    export function ros_connect(router_endpoint: string): void;

    /**
     * Drain one received /chatter message, or null.
     */
    export function ros_poll(): any;

    /**
     * Drain one status line ("CONNECTED" or "ERROR: ..."), or null.
     */
    export function ros_poll_status(): any;

    /**
     * Publish a std_msgs/String to /chatter.
     */
    export function ros_publish(text: string): boolean;

    /**
     * Initialize the threaded runtime. Returns false if SharedArrayBuffer is
     * unavailable (missing COOP/COEP headers).
     */
    export function ros_start(shim_url: string): boolean;

    /**
     * Automated test: runs on the main thread, drives the worker through the
     * channel API exactly like the interactive page does.
     */
    export function run_threaded_ros_test(): Promise<void>;

}
declare type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

declare interface InitOutput {
    readonly ros_connect: (a: number, b: number) => void;
    readonly ros_publish: (a: number, b: number) => number;
    readonly ros_start: (a: number, b: number) => number;
    readonly run_threaded_ros_test: () => any;
    readonly ros_poll: () => any;
    readonly ros_poll_status: () => any;
    readonly __zenoh_init_threaded_runtime: (a: number, b: number) => number;
    readonly __zenoh_worker_entry: (a: number) => void;
    readonly wasm_bindgen_33ec01efcb697a40___convert__closures_____invoke___wasm_bindgen_33ec01efcb697a40___JsValue__core_132a09007f6e6bcf___result__Result_____wasm_bindgen_33ec01efcb697a40___JsError___true_: (a: number, b: number, c: any) => [number, number];
    readonly wasm_bindgen_33ec01efcb697a40___convert__closures_____invoke___js_sys_d0ad465e0a9fce0e___Function_fn_wasm_bindgen_33ec01efcb697a40___JsValue_____wasm_bindgen_33ec01efcb697a40___sys__Undefined___js_sys_d0ad465e0a9fce0e___Function_fn_wasm_bindgen_33ec01efcb697a40___JsValue_____wasm_bindgen_33ec01efcb697a40___sys__Undefined_______true_: (a: number, b: number, c: any, d: any) => void;
    readonly wasm_bindgen_33ec01efcb697a40___convert__closures_____invoke___wasm_bindgen_33ec01efcb697a40___JsValue______true_: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen_33ec01efcb697a40___convert__closures_____invoke___js_sys_d0ad465e0a9fce0e___futures__task__wait_async_polyfill__MessageEvent______true_: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen_33ec01efcb697a40___convert__closures_____invoke___web_sys_512de71be2d150bc___features__gen_ErrorEvent__ErrorEvent______true_: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen_33ec01efcb697a40___convert__closures_____invoke___web_sys_512de71be2d150bc___features__gen_ErrorEvent__ErrorEvent______true__4: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen_33ec01efcb697a40___convert__closures_____invoke_______true_: (a: number, b: number) => void;
    readonly memory: WebAssembly.Memory;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_destroy_closure: (a: number, b: number) => void;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_thread_destroy: (a?: number, b?: number, c?: number) => void;
    readonly __wbindgen_start: (a: number) => void;
}

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput>, memory?: WebAssembly.Memory, thread_stack_size?: number }} module_or_path - Passing `InitInput` directly is deprecated.
 * @param {WebAssembly.Memory} memory - Deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
declare function wasm_bindgen (module_or_path?: { module_or_path: InitInput | Promise<InitInput>, memory?: WebAssembly.Memory, thread_stack_size?: number } | InitInput | Promise<InitInput>, memory?: WebAssembly.Memory): Promise<InitOutput>;
