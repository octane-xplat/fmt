/**
 * `@octane-xplat/fmt` — reusable in-process formatter for Octane Xplat
 * projects. One `Formatter` compiles the blessed lint-fix preset and resolved
 * format options once, then formats caller-owned text without touching the
 * filesystem or spawning any tool.
 */

export interface FormatterOptions {
	/** Directory lint/format configuration discovery starts from. */
	cwd: string;
	/** Explicit `.oxlintrc` path; otherwise discovered upward from `cwd`. */
	lintConfigPath?: string;
	/** Explicit `.oxfmtrc` path; otherwise discovered upward from `cwd`. */
	formatConfigPath?: string;
	/**
	 * CamelCase `.oxfmtrc`-shaped options applied above file configuration —
	 * the layer explicit CLI options ride on.
	 */
	formatOptions?: Record<string, unknown>;
}

export type FormatStage =
	| "config"
	| "parse"
	| "lint-fix"
	| "format"
	| "custom-fix"
	| "convergence";

export interface FormatDiagnostic {
	path: string;
	stage: FormatStage;
	/** Stable machine-readable code, e.g. `OXC_PARSE`, `TSRX_PROJECTION`. */
	code: string;
	message: string;
	/** One-based line in the authored source. */
	line?: number;
	/** One-based character column in the authored source. */
	column?: number;
}

export type FormatResult =
	| { ok: true; code: string; changed: boolean }
	| { ok: false; diagnostics: FormatDiagnostic[] };

export interface Formatter {
	/**
	 * Formats caller-owned `source` as if it lived at `path` (the path drives
	 * extension detection and per-file configuration overrides). Never reads
	 * or writes the file.
	 */
	formatText(input: { path: string; source: string }): Promise<FormatResult>;
	/** Whether the engine's ignore sets cover `path`. */
	shouldIgnore(path: string): boolean;
}

/**
 * Builds a formatter for `options.cwd`. Rejects when configuration is
 * invalid or unsupported.
 */
export declare function createFormatter(
	options: FormatterOptions,
): Promise<Formatter>;
