import { create } from 'zustand';
import { backend, type UpdateInfo } from '@/api';

/** Where the in-app update is: nothing known, a newer version waiting, or one being installed. */
export type UpdateState =
  | { phase: 'idle' }
  | { phase: 'checking' }
  | { phase: 'available'; info: UpdateInfo }
  /** The app restarts when the install finishes, so there is no "done" phase. */
  | { phase: 'installing'; info: UpdateInfo }
  /** `info` is kept when an install failed, so the user can try again without checking again. */
  | { phase: 'error'; message: string; info?: UpdateInfo };

export type UpdateEvent =
  | { type: 'checkStarted' }
  | { type: 'checkFinished'; info: UpdateInfo | null }
  | { type: 'checkFailed'; message: string }
  | { type: 'installStarted' }
  | { type: 'installFailed'; message: string };

/** Pure state machine; the store below only feeds it events. */
export function reduceUpdate(state: UpdateState, event: UpdateEvent): UpdateState {
  switch (event.type) {
    case 'checkStarted':
      // A check never interrupts an install, and never throws away a version already found.
      return state.phase === 'installing' || state.phase === 'checking' || state.phase === 'available' ? state : { phase: 'checking' };
    case 'checkFinished':
      if (state.phase === 'installing') return state;
      return event.info ? { phase: 'available', info: event.info } : { phase: 'idle' };
    case 'checkFailed':
      return state.phase === 'installing' ? state : { phase: 'error', message: event.message };
    case 'installStarted':
      if (state.phase === 'available') return { phase: 'installing', info: state.info };
      return state.phase === 'error' && state.info ? { phase: 'installing', info: state.info } : state;
    case 'installFailed':
      return state.phase === 'installing' ? { phase: 'error', message: event.message, info: state.info } : state;
  }
}

/** The version a "更新" click would install, if one is known. */
export function updatableVersion(state: UpdateState): UpdateInfo | null {
  return state.phase === 'available' || state.phase === 'installing' || (state.phase === 'error' && state.info) ? (state.info ?? null) : null;
}

interface UpdateStore {
  state: UpdateState;
  /** Never rejects: a failed check becomes the `error` state. */
  check: () => Promise<void>;
  install: () => Promise<void>;
}

const message = (error: unknown) => (error instanceof Error ? error.message : String(error));

export const useUpdate = create<UpdateStore>((set, get) => {
  const dispatch = (event: UpdateEvent) => set((s) => ({ state: reduceUpdate(s.state, event) }));
  return {
    state: { phase: 'idle' },
    async check() {
      if (get().state.phase === 'checking') return;
      dispatch({ type: 'checkStarted' });
      // 'available' keeps its version and skips the network round trip.
      if (get().state.phase !== 'checking') return;
      try {
        dispatch({ type: 'checkFinished', info: await backend.checkUpdate() });
      } catch (error) {
        dispatch({ type: 'checkFailed', message: message(error) });
      }
    },
    async install() {
      dispatch({ type: 'installStarted' });
      if (get().state.phase !== 'installing') return;
      try {
        await backend.installUpdate();
      } catch (error) {
        dispatch({ type: 'installFailed', message: message(error) });
      }
    },
  };
});
