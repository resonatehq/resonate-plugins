// Reference binding, TypeScript. One file per plugin, generated from
// plugins/<name>/spec/specification.md. The example below is Bannerbear;
// replace the names, types and target with the specification's own.

/** A plugin operation: what `ctx.rpc` needs where a function pointer would
 *  otherwise go. Mirrors the promise wire format — `func` is the §4 heading,
 *  `target` is the §1 address. */
export interface Integration<A, R> {
  readonly kind: "integration";
  readonly func: string;
  readonly target: string;
  /** Phantom carriers: erased at runtime, they give `ctx.rpc` its types. */
  readonly __args?: A;
  readonly __value?: R;
}

function op<A, R>(func: string, target: string): Integration<A, R> {
  return { kind: "integration", func, target };
}

/** §1 Address. `bannerbear://` selects the [bannerbear] config section;
 *  an instance name selects [bannerbear.<instance>]. */
export const address = (instance = "default") => `bannerbear://${instance}`;

// ── §2 Configuration ────────────────────────────────────────────────────
// What the server needs in its config section. Never sent by the caller —
// declared here so a caller can see what an operator must provide.
export interface Config {
  /** API key. No default: the operator must set it. */
  api_key: string;
  /** Poll cadence for `call + poll` operations. Default 2s. */
  poll?: string;
}

// ── §4 Operations ───────────────────────────────────────────────────────

/** §4.1.1 — the caller's vocabulary, not the provider's wire shape. */
export interface ImageCreateArgs {
  template: string;
  modifications: Array<{ name: string; text?: string; image_url?: string }>;
  metadata?: string;
  webhook_url?: string;
}

/** §4.1.2 Resolved. */
export interface ImageCreateValue {
  uid: string;
  status: string;
  image_url: string | null;
}

/** §4.1.2 Rejected — one member per `code` in the enum, so a caller can
 *  branch without matching strings. */
export type ImageCreateError =
  | { code: "not_found"; detail?: unknown }
  | { code: "invalid_request"; detail?: unknown }
  | { code: "render_failed"; detail?: unknown };

export const image = {
  /** Render an image and wait for it. `call + poll` in the specification —
   *  the waiting happens on the server; nothing here reflects it. */
  create: op<ImageCreateArgs, ImageCreateValue>("image.create", address()),
  /** Hand the render to the provider and resolve on acceptance. */
  submit: op<ImageCreateArgs, { uid: string; status: string }>("image.submit", address()),
  /** A plain read of the record. */
  get: op<{ uid: string }, ImageCreateValue>("image.get", address()),
} as const;

// Call site, for reference — identical in shape to a local call:
//
//   import * as bannerbear from "@resonatehq/plugin-bannerbear";
//   const img = await ctx.rpc(bannerbear.image.create, {
//     template: "A9xY...", modifications: [{ name: "title", text: "Hello" }],
//   });
