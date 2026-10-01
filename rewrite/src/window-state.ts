import type { PresentationState } from './contracts';

export function defaultPresentation(): PresentationState {
  return {
    split_basis_points: 4200, task_height: 240, task_open: false,
    task_history: false, inspector_tab: 'overview', current_movie: null, current_tv: null,
  };
}

// Keep the preferred ratio unchanged when a small viewport temporarily clamps it.
export function splitPixels(width: number, preference: number): number {
  const right = width <= 780 ? 260 : width <= 1120 ? 320 : 420;
  const available = Math.max(0, width - 12);
  const minimum = Math.min(330, available);
  return Math.max(minimum, Math.min(Math.max(minimum, available - right), width * preference / 10000));
}

export function taskPixels(height: number, preference: number): number {
  return Math.max(190, Math.min(height * 0.7, preference));
}
