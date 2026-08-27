export type AutosavePhase =
  | "idle"
  | "scheduled"
  | "saving"
  | "saved"
  | "error"
  | "disposed";

export interface AutosaveStatus {
  phase: AutosavePhase;
  dirtyRevision: number;
  savedRevision: number;
  savedAtMs?: number;
  error?: unknown;
}

export interface AutosaveWriter<TDocument> {
  (document: TDocument, reason: string): Promise<unknown>;
}

export interface AutosaveOptions<TDocument> {
  write: AutosaveWriter<TDocument>;
  delayMs?: number;
  maxWaitMs?: number;
  clone?: (document: TDocument) => TDocument;
  onStatus?: (status: AutosaveStatus) => void;
}

export interface AutosaveCommit {
  readonly revision: number;
  commit(): void;
  rollback(): void;
}

interface PendingDocument<TDocument> {
  document: TDocument;
  revision: number;
}

/**
 * Debounced autosave with a maximum wait and serialized writes. Changes made
 * during an in-flight write remain pending and are flushed immediately after it.
 */
export class AutosaveController<TDocument> {
  private readonly write: AutosaveWriter<TDocument>;
  private readonly delayMs: number;
  private readonly maxWaitMs: number;
  private readonly clone: (document: TDocument) => TDocument;
  private readonly onStatus?: (status: AutosaveStatus) => void;
  private debounceTimer: ReturnType<typeof setTimeout> | undefined;
  private maxWaitTimer: ReturnType<typeof setTimeout> | undefined;
  private pending: PendingDocument<TDocument> | undefined;
  private inFlight: Promise<void> | undefined;
  private revision = 0;
  private savedRevision = 0;
  private savedAtMs: number | undefined;
  private disposed = false;
  private lastError: unknown;
  private commitRevision: number | undefined;

  constructor(options: AutosaveOptions<TDocument>) {
    this.write = options.write;
    this.delayMs = Math.max(50, options.delayMs ?? 1_500);
    this.maxWaitMs = Math.max(this.delayMs, options.maxWaitMs ?? 15_000);
    this.clone = options.clone ?? cloneDocument;
    this.onStatus = options.onStatus;
    this.emit("idle");
  }

  markDirty(document: TDocument): number {
    this.assertActive();
    this.revision += 1;
    this.pending = {
      document: this.clone(document),
      revision: this.revision,
    };
    this.schedule();
    return this.revision;
  }

  get currentRevision(): number {
    return this.revision;
  }

  /**
   * Pauses recovery writes while a durable project save is in progress. The
   * caller commits the captured revision only after the project write succeeds;
   * newer edits remain pending and are autosaved after the barrier is released.
   */
  async beginCommit(revision = this.revision): Promise<AutosaveCommit> {
    this.assertActive();
    if (!Number.isInteger(revision) || revision < 0 || revision > this.revision) {
      throw new RangeError("Autosave commit revision is out of range");
    }
    if (this.commitRevision !== undefined) {
      throw new Error("An autosave commit is already in progress");
    }

    this.commitRevision = revision;
    this.clearTimers();
    if (this.inFlight) {
      try {
        await this.inFlight;
      } catch {
        // A successful project save supersedes a failed recovery write. On
        // rollback the failed revision remains pending and will be retried.
      }
    }

    let settled = false;
    const settle = (committed: boolean) => {
      if (settled) return;
      settled = true;
      this.finishCommit(revision, committed);
    };
    return {
      revision,
      commit: () => settle(true),
      rollback: () => settle(false),
    };
  }

  async flush(reason = "manual"): Promise<void> {
    if (this.disposed) return;
    if (this.commitRevision !== undefined) {
      if (this.inFlight) {
        try {
          await this.inFlight;
        } catch {
          // The commit owner decides whether to supersede or retry this write.
        }
      }
      return;
    }
    this.clearTimers();

    if (this.inFlight) {
      await this.inFlight;
      if (this.pending) await this.flush(reason);
      return;
    }
    if (!this.pending) return;

    const saving = this.pending;
    this.pending = undefined;
    this.lastError = undefined;
    this.emit("saving");
    const operation = this.write(saving.document, reason)
      .then(() => {
        this.savedRevision = Math.max(this.savedRevision, saving.revision);
        this.savedAtMs = Date.now();
        this.lastError = undefined;
        this.emit(this.pending ? "scheduled" : "saved", this.savedAtMs);
      })
      .catch((error: unknown) => {
        this.lastError = error;
        if (!this.pending || this.pending.revision < saving.revision) {
          this.pending = saving;
        }
        this.emit("error");
        throw error;
      })
      .finally(() => {
        this.inFlight = undefined;
      });
    this.inFlight = operation;
    await operation;

    if (this.pending && this.commitRevision === undefined) {
      await this.flush("changes-during-save");
    }
  }

  getStatus(): AutosaveStatus {
    return {
      phase: this.disposed
        ? "disposed"
        : this.inFlight
          ? "saving"
          : this.lastError
            ? "error"
            : this.pending
              ? "scheduled"
              : this.savedRevision > 0
                ? "saved"
                : "idle",
      dirtyRevision: this.revision,
      savedRevision: this.savedRevision,
      savedAtMs: this.savedAtMs,
      error: this.lastError,
    };
  }

  async dispose(options: { flush?: boolean } = {}): Promise<void> {
    if (this.disposed) return;
    if (options.flush) {
      try {
        await this.flush("dispose");
      } catch {
        // Caller can inspect the final status; disposal must always release timers.
      }
    }
    this.clearTimers();
    this.disposed = true;
    this.emit("disposed");
  }

  private schedule(): void {
    if (this.commitRevision !== undefined) {
      this.emit("scheduled");
      return;
    }
    if (this.debounceTimer) clearTimeout(this.debounceTimer);
    this.debounceTimer = setTimeout(() => {
      this.debounceTimer = undefined;
      void this.flush("debounce").catch(() => undefined);
    }, this.delayMs);

    if (!this.maxWaitTimer) {
      this.maxWaitTimer = setTimeout(() => {
        this.maxWaitTimer = undefined;
        void this.flush("max-wait").catch(() => undefined);
      }, this.maxWaitMs);
    }
    this.emit("scheduled");
  }

  private clearTimers(): void {
    if (this.debounceTimer) clearTimeout(this.debounceTimer);
    if (this.maxWaitTimer) clearTimeout(this.maxWaitTimer);
    this.debounceTimer = undefined;
    this.maxWaitTimer = undefined;
  }

  private assertActive(): void {
    if (this.disposed) throw new Error("AutosaveController has been disposed");
  }

  private finishCommit(revision: number, committed: boolean): void {
    if (this.commitRevision !== revision) {
      throw new Error("Autosave commit token is no longer active");
    }
    this.commitRevision = undefined;

    if (committed) {
      if (this.pending && this.pending.revision <= revision) {
        this.pending = undefined;
      }
      this.savedRevision = Math.max(this.savedRevision, revision);
      this.savedAtMs = Date.now();
      this.lastError = undefined;
    }

    if (this.disposed) {
      this.emit("disposed");
    } else if (this.pending) {
      this.schedule();
    } else if (this.lastError) {
      this.emit("error");
    } else {
      this.emit(this.savedRevision > 0 ? "saved" : "idle", this.savedAtMs);
    }
  }

  private emit(phase: AutosavePhase, savedAtMs?: number): void {
    this.onStatus?.({
      phase,
      dirtyRevision: this.revision,
      savedRevision: this.savedRevision,
      savedAtMs: savedAtMs ?? this.savedAtMs,
      error: this.lastError,
    });
  }
}

export function bindAutosaveLifecycle<TDocument>(
  controller: AutosaveController<TDocument>,
): () => void {
  if (typeof window === "undefined" || typeof document === "undefined") {
    return () => undefined;
  }

  const flushHidden = () => {
    if (document.visibilityState === "hidden") {
      void controller.flush("visibility-hidden").catch(() => undefined);
    }
  };
  const flushPage = () => {
    void controller.flush("page-hide").catch(() => undefined);
  };
  document.addEventListener("visibilitychange", flushHidden);
  window.addEventListener("pagehide", flushPage);

  return () => {
    document.removeEventListener("visibilitychange", flushHidden);
    window.removeEventListener("pagehide", flushPage);
  };
}

function cloneDocument<T>(document: T): T {
  if (typeof structuredClone === "function") {
    try {
      return structuredClone(document);
    } catch {
      // Reactive UI state can be a Proxy; project documents are JSON-safe.
    }
  }
  return JSON.parse(JSON.stringify(document)) as T;
}
