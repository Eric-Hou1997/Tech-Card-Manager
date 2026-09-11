// Generated from Rust contracts. Do not edit.
export type Space = "movie" | "tv";
export type Locale = "zh-CN" | "zh-Hant" | "en-US";
export type LibraryRoot = { id: string, space: Space, path: string, };
export type Configuration = { revision: number, locale: Locale, roots: Array<LibraryRoot>, };
export type TaskState = "requested" | "running" | "paused" | "cancelled" | "interrupted" | "failed" | "completed";
export type AppError = { code: string, message: string, path: string | null, operation_id: string | null, retryable: boolean, };
export type ScanRequest = { operation_id: string, space: Space, root_ids: Array<string>, };
export type TaskControl = { operation_id: string, task_id: string, state: TaskState, };
export type Task = { id: string, state: TaskState, locale: Locale, space: Space, roots: Array<LibraryRoot>, attempt: number, processed: number, errors: number, current_path: string | null, failure: AppError | null, };
export type Ownership = "external" | "generated" | "manual";
export type Tag = { value: string, ownership: Ownership, engine: string, };
export type MediaItem = { parser_revision: number, id: string, root_id: string, space: Space, path: string, source_hash: string, title: string, year: string, imdb: string, kind: string, season: string, episode: string, specs: { [key in string]?: Array<string> }, tags: Array<Tag>, error: AppError | null, };
export type CatalogQuery = { space: Space, search: string, only_errors: boolean, offset: number, limit: number, };
export type CatalogPage = { total: number, items: Array<MediaItem>, };
export type OperationResult = { "kind": "configuration", "result": Configuration } | { "kind": "task", "result": Task };
