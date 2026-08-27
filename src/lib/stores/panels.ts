// Panel System Types and Stores
// ================================
// Modular panel management for resizable, detachable UI panels

import { writable, derived, type Writable } from 'svelte/store';
import { browser } from '$app/environment';

// ═══════════════════════════════════════════
// TYPES
// ═══════════════════════════════════════════

export interface PanelConfig {
    id: string;
    title: string;
    minWidth: number;
    maxWidth: number;
    defaultWidth: number;
    isVisible: boolean;
    isDetached: boolean;
    position?: { x: number; y: number };
    size?: { width: number; height: number };
}

export interface LayoutState {
    sidebarWidth: number;
    inspectorWidth: number;
    timelineHeight: number;
}

export interface DetachedPanelState {
    id: string;
    x: number;
    y: number;
    width: number;
    height: number;
}

// ═══════════════════════════════════════════
// DEFAULT CONFIGURATIONS
// ═══════════════════════════════════════════

export const DEFAULT_LAYOUT: LayoutState = {
    sidebarWidth: 240,
    inspectorWidth: 240,
    timelineHeight: 220
};

export const PANEL_CONSTRAINTS = {
    sidebar: { min: 180, max: 400 },
    inspector: { min: 180, max: 400 },
    timeline: { min: 120, max: 400 }
};

const STORAGE_KEY = 'astral-editor-layout';
const DETACHED_STORAGE_KEY = 'astral-editor-detached-panels';

// ═══════════════════════════════════════════
// PERSISTENCE HELPERS
// ═══════════════════════════════════════════

function loadFromStorage<T>(key: string, defaultValue: T): T {
    if (!browser) return defaultValue;
    try {
        const stored = localStorage.getItem(key);
        if (stored) {
            return JSON.parse(stored);
        }
    } catch (e) {
        console.warn('Failed to load from localStorage:', e);
    }
    return defaultValue;
}

function saveToStorage<T>(key: string, value: T): void {
    if (!browser) return;
    try {
        localStorage.setItem(key, JSON.stringify(value));
    } catch (e) {
        console.warn('Failed to save to localStorage:', e);
    }
}

// ═══════════════════════════════════════════
// STORES
// ═══════════════════════════════════════════

// Layout dimensions store with persistence
const initialLayout = loadFromStorage<LayoutState>(STORAGE_KEY, DEFAULT_LAYOUT);
export const layoutStore: Writable<LayoutState> = writable(initialLayout);

// Auto-save layout changes
layoutStore.subscribe(value => {
    saveToStorage(STORAGE_KEY, value);
});

// Panel visibility store
export const panelVisibility = writable({
    sidebar: true,
    inspector: true,
    timeline: true,
    toolbar: true
});

// Detached panels store with positions
const initialDetached = loadFromStorage<Record<string, DetachedPanelState>>(DETACHED_STORAGE_KEY, {});
export const detachedPanels = writable<Record<string, DetachedPanelState>>(initialDetached);

// Auto-save detached panel states
detachedPanels.subscribe(value => {
    saveToStorage(DETACHED_STORAGE_KEY, value);
});

// Currently resizing panel
export const resizingPanel = writable<string | null>(null);

// ═══════════════════════════════════════════
// ACTIONS
// ═══════════════════════════════════════════

export function updatePanelWidth(panel: 'sidebar' | 'inspector', width: number) {
    const constraints = PANEL_CONSTRAINTS[panel];
    const clampedWidth = Math.max(constraints.min, Math.min(constraints.max, width));

    layoutStore.update(state => ({
        ...state,
        [`${panel}Width`]: clampedWidth
    }));
}

export function updateTimelineHeight(height: number) {
    const { min, max } = PANEL_CONSTRAINTS.timeline;
    const clampedHeight = Math.max(min, Math.min(max, height));

    layoutStore.update(state => ({
        ...state,
        timelineHeight: clampedHeight
    }));
}

export function togglePanel(panel: 'sidebar' | 'inspector' | 'timeline' | 'toolbar') {
    panelVisibility.update(state => ({
        ...state,
        [panel]: !state[panel]
    }));
}

export function detachPanel(panelId: string, x = 100, y = 100, width = 400, height = 350) {
    detachedPanels.update(panels => ({
        ...panels,
        [panelId]: { id: panelId, x, y, width, height }
    }));
}

export function attachPanel(panelId: string) {
    detachedPanels.update(panels => {
        const newPanels = { ...panels };
        delete newPanels[panelId];
        return newPanels;
    });
}

export function updateDetachedPanelPosition(panelId: string, x: number, y: number) {
    detachedPanels.update(panels => {
        if (!panels[panelId]) return panels;
        return {
            ...panels,
            [panelId]: { ...panels[panelId], x, y }
        };
    });
}

export function updateDetachedPanelSize(panelId: string, width: number, height: number) {
    detachedPanels.update(panels => {
        if (!panels[panelId]) return panels;
        return {
            ...panels,
            [panelId]: { ...panels[panelId], width, height }
        };
    });
}

export function isDetached(panelId: string): boolean {
    let result = false;
    detachedPanels.subscribe(panels => {
        result = panelId in panels;
    })();
    return result;
}

export function resetLayout() {
    layoutStore.set({ ...DEFAULT_LAYOUT });
    panelVisibility.set({
        sidebar: true,
        inspector: true,
        timeline: true,
        toolbar: true
    });
    detachedPanels.set({});

    // Clear localStorage
    if (browser) {
        localStorage.removeItem(STORAGE_KEY);
        localStorage.removeItem(DETACHED_STORAGE_KEY);
    }
}
