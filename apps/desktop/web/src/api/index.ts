import { mockBackend } from './mock';
import { tauriBackend } from './tauri';
import type { Backend } from './types';

const insideTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** The real Rust core inside the desktop app, an in-browser stand-in elsewhere. */
export const backend: Backend = insideTauri ? tauriBackend : mockBackend;

export * from './types';
