import { writable } from 'svelte/store';

export interface PointerDragState {
    file: string | null;
    pointerId: number | null;
    startX: number;
    startY: number;
    hasMoved: boolean;
    active: boolean;
}

export interface MediaDropFeedback {
    allowed: boolean;
    message?: string;
}

export const draggedMediaFile = writable<string | null>(null);
export const mediaDropFeedback = writable<MediaDropFeedback | null>(null);
export const pointerDragState = writable<PointerDragState>({
    file: null,
    pointerId: null,
    startX: 0,
    startY: 0,
    hasMoved: false,
    active: false
});

export function beginMediaDrag(path: string): void {
    draggedMediaFile.set(path);
    mediaDropFeedback.set(null);
}

export function endMediaDrag(): void {
    draggedMediaFile.set(null);
    mediaDropFeedback.set(null);
}

export function setMediaDropFeedback(feedback: MediaDropFeedback | null): void {
    mediaDropFeedback.set(feedback);
}

export function beginPointerDrag(
    path: string,
    pointerId: number,
    startX: number,
    startY: number
): void {
    draggedMediaFile.set(path);
    mediaDropFeedback.set(null);
    pointerDragState.set({
        file: path,
        pointerId,
        startX,
        startY,
        hasMoved: false,
        active: true
    });
}

export function markPointerDragMoved(): void {
    pointerDragState.update(state => {
        if (!state.active || state.hasMoved) return state;
        return { ...state, hasMoved: true };
    });
}

export function endPointerDrag(): void {
    pointerDragState.set({
        file: null,
        pointerId: null,
        startX: 0,
        startY: 0,
        hasMoved: false,
        active: false
    });
    draggedMediaFile.set(null);
    mediaDropFeedback.set(null);
}
