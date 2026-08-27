<script lang="ts">
    import { open as openDialog } from "@tauri-apps/plugin-dialog";
    import {
        decibelsToGain,
        gainToDecibels,
        type TextAnimationType,
        type TextClipStyle,
        type VoiceRiderMetadata,
    } from "$lib/editor/timeline-engine";
    import {
        TEXT_STYLE_PRESETS,
        type TextStylePreset,
    } from "$lib/editor/text-presets";
    import {
        DEFAULT_NOISE_REDUCTION,
        type NoiseReductionSettings,
    } from "$lib/editor/noise-reduction";
    import type {
        AudioEnhancementProgress,
        AudioEnhancementProgressPhase,
    } from "$lib/render/types";


    interface ClipTransform {
        x: number;
        y: number;
        scale: number;
        rotation: number;
        opacity: number;
        shake?: number;
    }

    interface TrackMix {
        gain: number;
        pan: number;
    }

    interface QuietRegionView {
        start: number;
        end: number;
        volume: number;
    }

    interface AudioRegionView {
        selection: { start: number; end: number; volume: number } | null;
        selectMode: boolean;
        quietRegions: QuietRegionView[];
    }

    interface Props {
        transform: ClipTransform;
        volume?: number;
        clipKind?: "video" | "audio" | "image" | "text";
        text?: TextClipStyle | null;
        disabled?: boolean;
        cameraAutomationActive?: boolean;
        audioSeparated?: boolean;
        speed?: number | null;
        trackMix?: TrackMix | null;
        audioRegion?: AudioRegionView | null;
        audioNormalizeBusy?: boolean;
        audioNormalizeMessage?: string | null;
        audioNormalizeError?: boolean;
        voiceRider?: VoiceRiderMetadata | null;
        voiceRiderCurrentGain?: number;
        effectiveVolume?: number;
        noiseReduction?: NoiseReductionSettings | null;
        noiseReductionBusy?: boolean;
        noiseReductionMessage?: string | null;
        noiseReductionError?: boolean;
        noiseComparisonAvailable?: boolean;
        noiseAuditionMode?: "original" | "cleaned" | "mossformer";
        mossFormerAvailable?: boolean;
        mossFormerBusy?: boolean;
        mossFormerProgress?: AudioEnhancementProgress | null;
        mossFormerMessage?: string | null;
        mossFormerError?: boolean;
        voiceRiderBusy?: boolean;
        voiceRiderMessage?: string | null;
        voiceRiderError?: boolean;
        onUpdate: (key: keyof ClipTransform | "volume", value: number) => void;
        onTextUpdate?: (updates: Partial<TextClipStyle>) => void;
        onResetPosition: () => void;
        onResetTransform: () => void;
        onSpeedChange?: (speed: number) => void;
        onTrackMixChange?: (key: "gain" | "pan", value: number) => void;
        onAddKeyframe?: (property: "opacity" | "volume") => void;
        onDelete?: () => void;
        onAudioRegionToggle?: () => void;
        onAudioRegionVolumeChange?: (volume: number) => void;
        onAudioRegionApply?: () => void;
        onAudioRegionRemove?: () => void;
        onAudioRegionCancel?: () => void;
        onQuietRegionOpen?: (region: QuietRegionView) => void;
        onNormalizeAudio?: () => void;
        onNoiseReductionChange?: (settings: NoiseReductionSettings | null) => void;
        onNoiseReductionRetry?: () => void;
        onNoiseAuditionModeChange?: (
            mode: "original" | "cleaned" | "mossformer",
        ) => void;
        onMossFormerPrepare?: () => void;
        onVoiceRider?: () => void;
        onVoiceRiderRemove?: () => void;
    }

    let {
        transform,
        volume = 1,
        clipKind = "video",
        text = null,
        disabled = false,
        cameraAutomationActive = false,
        audioSeparated = false,
        speed = null,
        trackMix = null,
        audioRegion = null,
        audioNormalizeBusy = false,
        audioNormalizeMessage = null,
        audioNormalizeError = false,
        voiceRider = null,
        voiceRiderCurrentGain = 1,
        effectiveVolume = 1,
        noiseReduction = null,
        noiseReductionBusy = false,
        noiseReductionMessage = null,
        noiseReductionError = false,
        noiseComparisonAvailable = false,
        noiseAuditionMode = "cleaned",
        mossFormerAvailable = false,
        mossFormerBusy = false,
        mossFormerProgress = null,
        mossFormerMessage = null,
        mossFormerError = false,
        voiceRiderBusy = false,
        voiceRiderMessage = null,
        voiceRiderError = false,
        onUpdate,
        onTextUpdate,
        onResetPosition,
        onResetTransform,
        onSpeedChange,
        onTrackMixChange,
        onAddKeyframe,
        onDelete,
        onAudioRegionToggle,
        onAudioRegionVolumeChange,
        onAudioRegionApply,
        onAudioRegionRemove,
        onAudioRegionCancel,
        onQuietRegionOpen,
        onNormalizeAudio,
        onNoiseReductionChange,
        onNoiseReductionRetry,
        onNoiseAuditionModeChange,
        onMossFormerPrepare,
        onVoiceRider,
        onVoiceRiderRemove,
    }: Props = $props();

    let hasVisualControls = $derived(clipKind !== "audio");
    let hasAudioControls = $derived(
        (clipKind === "audio" || clipKind === "video") && !audioSeparated,
    );
    let clipKindLabel = $derived(
        clipKind === "text"
            ? "Metin"
            : clipKind === "image"
              ? "Görsel"
              : clipKind === "audio"
                ? "Ses"
                : "Video",
    );
    let noiseControlsDisabled = $derived(
        disabled || audioNormalizeBusy || voiceRiderBusy || noiseReductionBusy || mossFormerBusy,
    );
    let studioVoiceActive = $derived(
        mossFormerAvailable && noiseAuditionMode === "mossformer",
    );
    let mossFormerProgressPercent = $derived.by(() => {
        const value = mossFormerProgress?.overallProgressPercent;
        if (value === null || value === undefined || !Number.isFinite(value)) return null;
        return Math.max(0, Math.min(100, Math.round(value)));
    });
    let mossFormerPhaseProgressPercent = $derived.by(() => {
        const value = mossFormerProgress?.phaseProgressPercent;
        if (value === null || value === undefined || !Number.isFinite(value)) return null;
        return Math.max(0, Math.min(100, Math.round(value)));
    });
    let mossFormerProgressTime = $derived.by(() => {
        const processedMs = mossFormerProgress?.processedMs;
        if (processedMs === null || processedMs === undefined || !Number.isFinite(processedMs)) {
            return null;
        }
        const processed = formatTime(Math.max(0, processedMs) / 1_000);
        const totalMs = mossFormerProgress?.totalMs;
        return totalMs !== null && totalMs !== undefined && Number.isFinite(totalMs)
            ? `${processed} / ${formatTime(Math.max(0, totalMs) / 1_000)}`
            : `${processed} işlendi`;
    });

    function audioEnhancementPhaseLabel(
        phase: AudioEnhancementProgressPhase | undefined,
    ): string {
        switch (phase) {
            case "runtime-setup": return "AI çalışma ortamı";
            case "model-download": return "Modeller indiriliyor";
            case "preparing-audio": return "Ses hazırlanıyor";
            case "deepfilter": return "DeepFilterNet3";
            case "mossformer": return "MossFormer2";
            case "mastering": return "Stüdyo tonlaması";
            case "finalizing": return "Ses sonlandırılıyor";
            case "completed": return "Tamamlandı";
            default: return "Sırada";
        }
    }

    function toggleNoiseReduction() {
        onNoiseReductionChange?.(
            noiseReduction ? null : { ...DEFAULT_NOISE_REDUCTION },
        );
    }

    function toggleStudioVoice() {
        if (mossFormerBusy) return;
        if (studioVoiceActive) {
            onNoiseAuditionModeChange?.(
                noiseReduction && noiseComparisonAvailable ? "cleaned" : "original",
            );
            return;
        }
        if (mossFormerAvailable) {
            onNoiseAuditionModeChange?.("mossformer");
            return;
        }
        onMossFormerPrepare?.();
    }

    function formatSignedDb(multiplier: number): string {
        const value = gainToDecibels(multiplier);
        if (!Number.isFinite(value)) return "Sessiz";
        return `${value > 0 ? "+" : ""}${value.toFixed(1)} dB`;
    }

    const MIN_EDITABLE_GAIN_DB = -60;
    const MAX_EDITABLE_GAIN_DB = gainToDecibels(4);

    function gainDbSliderValue(gain: number): number {
        const value = gainToDecibels(gain);
        return Number.isFinite(value)
            ? Math.min(MAX_EDITABLE_GAIN_DB, Math.max(MIN_EDITABLE_GAIN_DB, value))
            : MIN_EDITABLE_GAIN_DB;
    }

    function formatGainReadout(gain: number): string {
        return `${formatSignedDb(gain)} · %${Math.round(Math.max(0, gain) * 100)}`;
    }

    function formatRelativeGainReadout(gain: number, reference: number): string {
        return formatGainReadout(reference > 0 ? gain / reference : 0);
    }
    let lastOpaqueBackground = $state("#000000");

    $effect(() => {
        const background = text?.backgroundColor;
        if (background && background !== "transparent" && isHexColor(background)) {
            lastOpaqueBackground = normalizeHexColor(background, "#000000");
        }
    });

    function handleChange(key: keyof ClipTransform | "volume", event: Event) {
        let value = (event.currentTarget as HTMLInputElement).valueAsNumber;
        if (!Number.isFinite(value)) return;
        
        // Snap settings for sliders to make it easy to hit exact defaults
        if (key === "rotation") {
            // Snap to 0 if within 4 degrees
            if (Math.abs(value) <= 4) value = 0;
            // Snap to 90 / -90 / 180 / -180 if within 3 degrees
            else if (Math.abs(value - 90) <= 3) value = 90;
            else if (Math.abs(value + 90) <= 3) value = -90;
            else if (Math.abs(value - 180) <= 3) value = 180;
            else if (Math.abs(value + 180) <= 3) value = -180;
        } else if (key === "scale") {
            // Snap to 1.0 (100% size) if within 0.05
            if (Math.abs(value - 1.0) <= 0.05) value = 1.0;
            else if (Math.abs(value - 2.0) <= 0.05) value = 2.0;
            else if (Math.abs(value - 0.5) <= 0.05) value = 0.5;
        } else if (key === "opacity") {
            // Snap to 1.0 (fully opaque) if within 0.03
            if (value >= 0.97) value = 1.0;
            else if (value <= 0.03) value = 0.0;
        } else if (key === "volume") {
            // Snap to 1.0 (100% volume) if within 0.05
            if (Math.abs(value - 1.0) <= 0.05) value = 1.0;
            else if (value <= 0.05) value = 0.0;
        } else if (key === "shake") {
            // Snap to 0 (no shake) if within 4%
            if (value <= 4) value = 0;
        }

        onUpdate(key, value);
    }

    function handleSpeedChange(event: Event) {
        const value = (event.currentTarget as HTMLInputElement).valueAsNumber;
        if (!Number.isFinite(value)) return;
        onSpeedChange?.(value);
    }

    function handleTrackMixChange(key: "gain" | "pan", event: Event) {
        const value = (event.currentTarget as HTMLInputElement).valueAsNumber;
        if (!Number.isFinite(value)) return;
        onTrackMixChange?.(key, value);
    }

    function handleVolumeDbChange(event: Event) {
        let decibels = (event.currentTarget as HTMLInputElement).valueAsNumber;
        if (!Number.isFinite(decibels)) return;
        if (decibels <= MIN_EDITABLE_GAIN_DB) {
            onUpdate("volume", 0);
            return;
        }
        if (Math.abs(decibels) <= 0.15) decibels = 0;
        onUpdate("volume", decibelsToGain(decibels));
    }

    function handleTrackGainDbChange(event: Event) {
        let decibels = (event.currentTarget as HTMLInputElement).valueAsNumber;
        if (!Number.isFinite(decibels)) return;
        if (decibels <= MIN_EDITABLE_GAIN_DB) {
            onTrackMixChange?.("gain", 0);
            return;
        }
        if (Math.abs(decibels) <= 0.15) decibels = 0;
        onTrackMixChange?.("gain", decibelsToGain(decibels));
    }

    function handleRegionVolumeInput(event: Event) {
        const value = (event.currentTarget as HTMLInputElement).valueAsNumber;
        if (!Number.isFinite(value)) return;
        onAudioRegionVolumeChange?.(value);
    }

    function formatTime(seconds: number): string {
        const mins = Math.floor(seconds / 60);
        const secs = Math.floor(seconds % 60);
        return `${mins}:${secs.toString().padStart(2, "0")}`;
    }

    function handleTextContent(event: Event) {
        onTextUpdate?.({
            content: (event.currentTarget as HTMLTextAreaElement).value,
        });
    }

    function handleFontSize(event: Event) {
        const fontSize = (event.currentTarget as HTMLInputElement).valueAsNumber;
        if (!Number.isFinite(fontSize)) return;
        onTextUpdate?.({ fontSize });
    }

    function handleFontWeight(event: Event) {
        const fontWeight = Number((event.currentTarget as HTMLSelectElement).value);
        if (!Number.isFinite(fontWeight)) return;
        onTextUpdate?.({ fontWeight });
    }

    function handleFontFamily(event: Event) {
        const fontFamily = (event.currentTarget as HTMLSelectElement).value;
        const updates: Partial<TextClipStyle> = { fontFamily };
        if (text?.fontPath && fontFamily !== text.fontFamily) {
            updates.fontPath = undefined;
        }
        onTextUpdate?.(updates);
    }

    async function handleFontUpload() {
        if (!onTextUpdate) return;
        try {
            const filePath = await openDialog({
                multiple: false,
                filters: [
                    { name: "Yazı Tipi (Font)", extensions: ["ttf", "otf", "woff", "woff2"] }
                ],
                title: "Özel Font Dosyası Seç"
            });

            if (filePath && typeof filePath === "string") {
                let fileName = filePath.split(/[/\\]/).pop() || "CustomFont";
                const dotIndex = fileName.lastIndexOf(".");
                if (dotIndex !== -1) {
                    fileName = fileName.substring(0, dotIndex);
                }
                const fontFamilyName = fileName.replace(/[^a-zA-Z0-9_-]/g, "_");
                
                onTextUpdate({
                    fontFamily: fontFamilyName,
                    fontPath: filePath
                });
            }
        } catch (error) {
            console.error("Failed to choose font file:", error);
        }
    }

    function handleTextColor(
        key: "color" | "backgroundColor",
        event: Event,
    ) {
        const value = (event.currentTarget as HTMLInputElement).value;
        onTextUpdate?.(
            key === "color" ? { color: value } : { backgroundColor: value },
        );
    }

    function setTextAlign(align: TextClipStyle["align"]) {
        onTextUpdate?.({ align });
    }

    function toggleBackgroundTransparency(event: Event) {
        const isTransparent = (event.currentTarget as HTMLInputElement).checked;
        onTextUpdate?.({
            backgroundColor: isTransparent ? "transparent" : lastOpaqueBackground,
        });
    }

    const TEXT_ANIMATION_OPTIONS: readonly {
        value: TextAnimationType;
        label: string;
    }[] = [
        { value: "none", label: "Yok" },
        { value: "fade", label: "Yumuşak görün" },
        { value: "slide-up", label: "Kaydır ↑" },
        { value: "slide-down", label: "Kaydır ↓" },
        { value: "slide-left", label: "Kaydır ←" },
        { value: "slide-right", label: "Kaydır →" },
        { value: "zoom", label: "Büyüyerek gel" },
        { value: "bounce", label: "Zıplama" },
        { value: "typewriter", label: "Daktilo" },
        { value: "flip-3d", label: "3D Çevir" },
        { value: "spin", label: "Dönerek" },
    ];

    let lastShadowColor = $state("#000000");

    $effect(() => {
        const shadow = text?.shadowColor;
        if (shadow && shadow !== "transparent" && isHexColor(shadow)) {
            lastShadowColor = normalizeHexColor(shadow, "#000000");
        }
    });

    function applyPreset(preset: TextStylePreset) {
        // Presets restyle the clip; a previously uploaded custom font would
        // otherwise override the preset's family on export.
        onTextUpdate?.({ ...preset.style, fontPath: undefined });
    }

    function handleStrokeWidth(event: Event) {
        const strokeWidth = (event.currentTarget as HTMLInputElement).valueAsNumber;
        if (!Number.isFinite(strokeWidth)) return;
        onTextUpdate?.({ strokeWidth: Math.max(0, strokeWidth) });
    }

    function handleStrokeColor(event: Event) {
        onTextUpdate?.({
            strokeColor: (event.currentTarget as HTMLInputElement).value,
        });
    }

    function toggleShadow(event: Event) {
        const enabled = (event.currentTarget as HTMLInputElement).checked;
        onTextUpdate?.(
            enabled
                ? {
                      shadowColor: lastShadowColor,
                      shadowBlur: text?.shadowBlur || 12,
                  }
                : { shadowColor: "transparent" },
        );
    }

    function handleShadowColor(event: Event) {
        onTextUpdate?.({
            shadowColor: (event.currentTarget as HTMLInputElement).value,
        });
    }

    function handleShadowNumber(
        key: "shadowBlur" | "shadowOffsetX" | "shadowOffsetY",
        event: Event,
    ) {
        const value = (event.currentTarget as HTMLInputElement).valueAsNumber;
        if (!Number.isFinite(value)) return;
        onTextUpdate?.({ [key]: value });
    }

    function toggleGradient(event: Event) {
        const enabled = (event.currentTarget as HTMLInputElement).checked;
        onTextUpdate?.({
            gradient: enabled
                ? {
                      from: normalizeHexColor(text?.color, "#ffffff"),
                      to: "#ff9d00",
                  }
                : null,
        });
    }

    function handleGradientColor(stop: "from" | "to", event: Event) {
        if (!text?.gradient) return;
        onTextUpdate?.({
            gradient: {
                ...text.gradient,
                [stop]: (event.currentTarget as HTMLInputElement).value,
            },
        });
    }

    function handleTextAnimationType(side: "in" | "out", event: Event) {
        const type = (event.currentTarget as HTMLSelectElement)
            .value as TextAnimationType;
        const current = side === "in" ? text?.animationIn : text?.animationOut;
        const next = { type, duration: current?.duration ?? 0.6 };
        onTextUpdate?.(
            side === "in" ? { animationIn: next } : { animationOut: next },
        );
    }

    function handleTextAnimationDuration(side: "in" | "out", event: Event) {
        const duration = (event.currentTarget as HTMLInputElement).valueAsNumber;
        if (!Number.isFinite(duration)) return;
        const current = side === "in" ? text?.animationIn : text?.animationOut;
        const next = { type: current?.type ?? "none", duration };
        onTextUpdate?.(
            side === "in" ? { animationIn: next } : { animationOut: next },
        );
    }

    function isHexColor(value: string): boolean {
        return /^#[0-9a-f]{3}([0-9a-f]{3})?$/i.test(value);
    }

    function normalizeHexColor(value: string | undefined, fallback: string): string {
        if (!value || !isHexColor(value)) return fallback;
        if (value.length === 7) return value;
        return `#${value[1]}${value[1]}${value[2]}${value[2]}${value[3]}${value[3]}`;
    }
</script>

<div class="inspector">
    <header class="header">
        <h3 id="inspector-title">{clipKindLabel} özellikleri</h3>
        {#if onDelete}
            <button
                type="button"
                class="delete-button"
                {disabled}
                title="Seçili klibi sil"
                onclick={onDelete}
            >Sil</button>
        {/if}
    </header>

    {#if disabled}
        <p id="inspector-locked-note" class="locked-note" role="status">Kanal kilitli. Ayarları değiştirmek için önce kilidi açın.</p>
    {/if}

    {#if audioSeparated}
        <p class="separated-note" role="status">Bu klibin sesi ayrıldı ve Voice kanalına taşındı. Bu klip artık sessiz; isterseniz üzerine yeni bir ses ekleyebilirsiniz.</p>
    {/if}

    <fieldset
        class="inspector-fields"
        {disabled}
        aria-labelledby="inspector-title"
        aria-describedby={disabled ? "inspector-locked-note" : undefined}
    >

    {#if clipKind === "text"}
        <section class="section" aria-labelledby="text-section-title">
            <h4 id="text-section-title" class="section-title">Metin</h4>

            {#if text}
                <div class="control-group content-control">
                    <label for="text-content">İçerik</label>
                    <textarea
                        id="text-content"
                        rows="4"
                        value={text.content}
                        disabled={!onTextUpdate}
                        placeholder="Önizlemede görünecek metni yazın"
                        oninput={handleTextContent}
                    ></textarea>
                </div>

                <div class="control-group preset-control">
                    <span class="control-label">Şablonlar</span>
                    <div class="preset-grid" role="group" aria-label="Metin şablonları">
                        {#each TEXT_STYLE_PRESETS as preset (preset.id)}
                            <button
                                type="button"
                                class="preset-chip"
                                style={`color: ${preset.chip.color}; background: ${preset.chip.background};`}
                                disabled={!onTextUpdate}
                                title={`${preset.label} şablonunu uygula`}
                                onclick={() => applyPreset(preset)}
                            >{preset.label}</button>
                        {/each}
                    </div>
                </div>

                <div class="row font-family-row" style="margin-bottom: 15px;">
                    <div class="control-group" style="width: 100%;">
                        <label for="font-family">Yazı Tipi (Font)</label>
                        <div class="font-family-control" style="display: flex; gap: 8px;">
                            <select
                                id="font-family"
                                value={text.fontFamily}
                                disabled={!onTextUpdate}
                                onchange={handleFontFamily}
                                style="flex: 1;"
                            >
                                <optgroup label="Standart Fontlar">
                                    <option value="Inter">Inter (Varsayılan)</option>
                                    <option value="Arial">Arial</option>
                                    <option value="Arial Black">Arial Black</option>
                                    <option value="Segoe UI">Segoe UI</option>
                                    <option value="Bahnschrift">Bahnschrift</option>
                                    <option value="Impact">Impact</option>
                                    <option value="Georgia">Georgia</option>
                                    <option value="Times New Roman">Times New Roman</option>
                                    <option value="Courier New">Courier New</option>
                                    <option value="Consolas">Consolas</option>
                                    <option value="Verdana">Verdana</option>
                                    <option value="Tahoma">Tahoma</option>
                                    <option value="Trebuchet MS">Trebuchet MS</option>
                                    <option value="Comic Sans MS">Comic Sans MS</option>
                                </optgroup>
                                <optgroup label="Google Fontları">
                                    <option value="Roboto">Roboto</option>
                                    <option value="Montserrat">Montserrat</option>
                                    <option value="Playfair Display">Playfair Display</option>
                                    <option value="Lora">Lora</option>
                                    <option value="Oswald">Oswald</option>
                                    <option value="JetBrains Mono">JetBrains Mono</option>
                                </optgroup>
                                {#if text.fontPath}
                                    <optgroup label="Özel Font">
                                        <option value={text.fontFamily}>{text.fontFamily} (Dosyadan)</option>
                                    </optgroup>
                                {/if}
                            </select>
                            
                            <button
                                type="button"
                                class="font-upload-btn"
                                disabled={!onTextUpdate}
                                onclick={handleFontUpload}
                                title="Bilgisayarınızdan özel font yükleyin (.ttf, .otf, .woff)"
                                style="min-height: 36px; padding: 6px 12px; border: 1px dashed #343937; border-radius: 5px; background: #191b1a; color: #65d7b2; font: 600 10px/1 'Inter', sans-serif; cursor: pointer; display: flex; align-items: center; justify-content: center; transition: all 0.2s;"
                            >
                                {#if text.fontPath}Fontu Değiştir{:else}Özel Font Seç...{/if}
                            </button>
                        </div>
                        {#if text.fontPath}
                            <p class="hint" style="color: #65d7b2; margin-top: 4px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;" title={text.fontPath}>
                                Konum: {text.fontPath}
                            </p>
                        {/if}
                    </div>
                </div>

                <div class="row text-row">
                    <div class="control-group half">
                        <label for="font-size">Yazı boyutu</label>
                        <input
                            id="font-size"
                            type="number"
                            min="8"
                            max="512"
                            step="1"
                            value={text.fontSize}
                            disabled={!onTextUpdate}
                            oninput={handleFontSize}
                        />
                    </div>
                    <div class="control-group half">
                        <label for="font-weight">Kalınlık</label>
                        <select
                            id="font-weight"
                            value={text.fontWeight}
                            disabled={!onTextUpdate}
                            onchange={handleFontWeight}
                        >
                            <option value="300">İnce</option>
                            <option value="400">Normal</option>
                            <option value="500">Orta</option>
                            <option value="600">Yarı kalın</option>
                            <option value="700">Kalın</option>
                            <option value="800">Çok kalın</option>
                            <option value="900">Siyah</option>
                        </select>
                    </div>
                </div>

                <div class="row color-row">
                    <div class="control-group half">
                        <label for="text-color">Yazı rengi</label>
                        <div class="color-control">
                            <input
                                id="text-color"
                                type="color"
                                value={normalizeHexColor(text.color, "#ffffff")}
                                disabled={!onTextUpdate}
                                aria-label="Yazı rengini seç"
                                oninput={(event) => handleTextColor("color", event)}
                            />
                            <span>{normalizeHexColor(text.color, "#ffffff")}</span>
                        </div>
                    </div>
                    <div class="control-group half">
                        <label for="background-color">Arka plan</label>
                        <div class="color-control">
                            <input
                                id="background-color"
                                type="color"
                                value={normalizeHexColor(
                                    text.backgroundColor,
                                    lastOpaqueBackground,
                                )}
                                disabled={!onTextUpdate ||
                                    text.backgroundColor === "transparent"}
                                aria-label="Arka plan rengini seç"
                                oninput={(event) =>
                                    handleTextColor("backgroundColor", event)}
                            />
                            <span
                                >{text.backgroundColor === "transparent"
                                    ? "Yok"
                                    : normalizeHexColor(
                                          text.backgroundColor,
                                          lastOpaqueBackground,
                                      )}</span
                            >
                        </div>
                    </div>
                </div>

                <label class="transparency-toggle" for="transparent-background">
                    <input
                        id="transparent-background"
                        type="checkbox"
                        checked={text.backgroundColor === "transparent"}
                        disabled={!onTextUpdate}
                        onchange={toggleBackgroundTransparency}
                    />
                    <span>Arka planı şeffaf yap</span>
                </label>

                <div class="control-group alignment-control">
                    <span class="control-label">Hizalama</span>
                    <div class="alignment-buttons" role="group" aria-label="Metin hizalama">
                        <button
                            type="button"
                            class:active={text.align === "left"}
                            aria-pressed={text.align === "left"}
                            disabled={!onTextUpdate}
                            onclick={() => setTextAlign("left")}>Sol</button
                        >
                        <button
                            type="button"
                            class:active={text.align === "center"}
                            aria-pressed={text.align === "center"}
                            disabled={!onTextUpdate}
                            onclick={() => setTextAlign("center")}>Orta</button
                        >
                        <button
                            type="button"
                            class:active={text.align === "right"}
                            aria-pressed={text.align === "right"}
                            disabled={!onTextUpdate}
                            onclick={() => setTextAlign("right")}>Sağ</button
                        >
                    </div>
                </div>

                <h4 class="section-title subsection-title">Dış çizgi</h4>
                <div class="row">
                    <div class="control-group half">
                        <label for="stroke-width">Kalınlık</label>
                        <input
                            id="stroke-width"
                            type="number"
                            min="0"
                            max="20"
                            step="0.5"
                            value={text.strokeWidth ?? 0}
                            disabled={!onTextUpdate}
                            title="0 = dış çizgi yok"
                            oninput={handleStrokeWidth}
                        />
                    </div>
                    <div class="control-group half">
                        <label for="stroke-color">Renk</label>
                        <div class="color-control">
                            <input
                                id="stroke-color"
                                type="color"
                                value={normalizeHexColor(text.strokeColor, "#000000")}
                                disabled={!onTextUpdate || !(text.strokeWidth > 0)}
                                aria-label="Dış çizgi rengini seç"
                                oninput={handleStrokeColor}
                            />
                            <span>{(text.strokeWidth ?? 0) > 0
                                    ? normalizeHexColor(text.strokeColor, "#000000")
                                    : "Yok"}</span>
                        </div>
                    </div>
                </div>

                <h4 class="section-title subsection-title">Gölge / Parlama</h4>
                <label class="transparency-toggle" for="shadow-enabled">
                    <input
                        id="shadow-enabled"
                        type="checkbox"
                        checked={text.shadowColor !== "transparent"}
                        disabled={!onTextUpdate}
                        onchange={toggleShadow}
                    />
                    <span>Gölge veya parlama ekle</span>
                </label>
                {#if text.shadowColor !== "transparent"}
                    <div class="row">
                        <div class="control-group half">
                            <label for="shadow-color">Renk</label>
                            <div class="color-control">
                                <input
                                    id="shadow-color"
                                    type="color"
                                    value={normalizeHexColor(text.shadowColor, "#000000")}
                                    disabled={!onTextUpdate}
                                    aria-label="Gölge rengini seç"
                                    oninput={handleShadowColor}
                                />
                                <span>{normalizeHexColor(text.shadowColor, "#000000")}</span>
                            </div>
                        </div>
                        <div class="control-group half">
                            <label for="shadow-blur">Yayılma</label>
                            <input
                                id="shadow-blur"
                                type="number"
                                min="0"
                                max="100"
                                step="1"
                                value={text.shadowBlur ?? 0}
                                disabled={!onTextUpdate}
                                title="Yüksek değer + parlak renk = neon parlaması"
                                oninput={(event) => handleShadowNumber("shadowBlur", event)}
                            />
                        </div>
                    </div>
                    <div class="row">
                        <div class="control-group half">
                            <label for="shadow-offset-x">Kaydırma X</label>
                            <input
                                id="shadow-offset-x"
                                type="number"
                                min="-60"
                                max="60"
                                step="1"
                                value={text.shadowOffsetX ?? 0}
                                disabled={!onTextUpdate}
                                oninput={(event) => handleShadowNumber("shadowOffsetX", event)}
                            />
                        </div>
                        <div class="control-group half">
                            <label for="shadow-offset-y">Kaydırma Y</label>
                            <input
                                id="shadow-offset-y"
                                type="number"
                                min="-60"
                                max="60"
                                step="1"
                                value={text.shadowOffsetY ?? 0}
                                disabled={!onTextUpdate}
                                title="X/Y kaydırma ile 3D blok görünümü elde edilir"
                                oninput={(event) => handleShadowNumber("shadowOffsetY", event)}
                            />
                        </div>
                    </div>
                {/if}

                <h4 class="section-title subsection-title">Degrade dolgu</h4>
                <label class="transparency-toggle" for="gradient-enabled">
                    <input
                        id="gradient-enabled"
                        type="checkbox"
                        checked={Boolean(text.gradient)}
                        disabled={!onTextUpdate}
                        onchange={toggleGradient}
                    />
                    <span>İki renkli degrade kullan</span>
                </label>
                {#if text.gradient}
                    <div class="row">
                        <div class="control-group half">
                            <label for="gradient-from">Üst renk</label>
                            <div class="color-control">
                                <input
                                    id="gradient-from"
                                    type="color"
                                    value={normalizeHexColor(text.gradient.from, "#ffffff")}
                                    disabled={!onTextUpdate}
                                    aria-label="Degrade üst rengi"
                                    oninput={(event) => handleGradientColor("from", event)}
                                />
                                <span>{normalizeHexColor(text.gradient.from, "#ffffff")}</span>
                            </div>
                        </div>
                        <div class="control-group half">
                            <label for="gradient-to">Alt renk</label>
                            <div class="color-control">
                                <input
                                    id="gradient-to"
                                    type="color"
                                    value={normalizeHexColor(text.gradient.to, "#ff9d00")}
                                    disabled={!onTextUpdate}
                                    aria-label="Degrade alt rengi"
                                    oninput={(event) => handleGradientColor("to", event)}
                                />
                                <span>{normalizeHexColor(text.gradient.to, "#ff9d00")}</span>
                            </div>
                        </div>
                    </div>
                {/if}

                <h4 class="section-title subsection-title">Animasyon</h4>
                <div class="animation-row">
                    <label class="animation-label" for="text-anim-in">Giriş</label>
                    <select
                        id="text-anim-in"
                        value={text.animationIn?.type ?? "none"}
                        disabled={!onTextUpdate}
                        onchange={(event) => handleTextAnimationType("in", event)}
                    >
                        {#each TEXT_ANIMATION_OPTIONS as option (option.value)}
                            <option value={option.value}>{option.label}</option>
                        {/each}
                    </select>
                    <input
                        type="number"
                        min="0.1"
                        max="5"
                        step="0.1"
                        value={text.animationIn?.duration ?? 0.6}
                        disabled={!onTextUpdate ||
                            (text.animationIn?.type ?? "none") === "none"}
                        aria-label="Giriş animasyonu süresi (saniye)"
                        title="Süre (saniye)"
                        onchange={(event) => handleTextAnimationDuration("in", event)}
                    />
                </div>
                <div class="animation-row">
                    <label class="animation-label" for="text-anim-out">Çıkış</label>
                    <select
                        id="text-anim-out"
                        value={text.animationOut?.type ?? "none"}
                        disabled={!onTextUpdate}
                        onchange={(event) => handleTextAnimationType("out", event)}
                    >
                        {#each TEXT_ANIMATION_OPTIONS as option (option.value)}
                            <option value={option.value}>{option.label}</option>
                        {/each}
                    </select>
                    <input
                        type="number"
                        min="0.1"
                        max="5"
                        step="0.1"
                        value={text.animationOut?.duration ?? 0.6}
                        disabled={!onTextUpdate ||
                            (text.animationOut?.type ?? "none") === "none"}
                        aria-label="Çıkış animasyonu süresi (saniye)"
                        title="Süre (saniye)"
                        onchange={(event) => handleTextAnimationDuration("out", event)}
                    />
                </div>
            {:else}
                <p class="empty-state" role="status">Metin ayarları kullanılamıyor.</p>
            {/if}
        </section>
    {/if}

    {#if hasVisualControls}
        {#if clipKind === "text"}<div class="separator" aria-hidden="true"></div>{/if}

        <section class="section" aria-labelledby="transform-section-title">
            <div class="section-heading">
                <h4 id="transform-section-title" class="section-title">Dönüşüm</h4>
                <button
                    type="button"
                    class="reset-button"
                    onclick={onResetTransform}
                    disabled={cameraAutomationActive}
                    title="Konum, boyut, döndürme ve opaklığı başlangıç değerlerine döndür"
                >Tümünü sıfırla</button>
            </div>

            {#if cameraAutomationActive}
                <p class="camera-automation-note" role="status">Konum, boyut ve tuval sürükleme AI Kadraj kamera yolundan geliyor. Elle değiştirmek için AI Kadraj'ı kaldırın.</p>
            {/if}

            <div class="control-group">
                <label for="opacity">Opaklık</label>
                <div class="control range-control">
                    <input
                        id="opacity"
                        type="range"
                        min="0"
                        max="1"
                        step="0.01"
                        value={transform.opacity}
                        aria-valuetext={`${Math.round(transform.opacity * 100)}%`}
                        oninput={(event) => handleChange("opacity", event)}
                    />
                    <output class="value" for="opacity"
                        >{Math.round(transform.opacity * 100)}%</output
                    >
                </div>
            </div>

            <div class="control-group">
                <label for="scale">Boyut</label>
                <div class="control range-control">
                    <input
                        id="scale"
                        type="range"
                        min="0.1"
                        max="3"
                        step="0.01"
                        value={transform.scale}
                        disabled={cameraAutomationActive}
                        aria-valuetext={`${transform.scale.toFixed(2)} kat`}
                        oninput={(event) => handleChange("scale", event)}
                    />
                    <output class="value" for="scale"
                        >{transform.scale.toFixed(2)}×</output
                    >
                </div>
            </div>

            <div class="control-group">
                <label for="rotation">Döndürme</label>
                <div class="control range-control">
                    <input
                        id="rotation"
                        type="range"
                        min="-180"
                        max="180"
                        step="1"
                        value={transform.rotation}
                        disabled={cameraAutomationActive}
                        aria-valuetext={`${Math.round(transform.rotation)} derece`}
                        oninput={(event) => handleChange("rotation", event)}
                    />
                    <output class="value" for="rotation"
                        >{Math.round(transform.rotation)}°</output
                    >
                </div>
            </div>

            <div class="position-heading">
                <span>Konum</span>
                <button
                    type="button"
                    class="reset-button compact"
                    onclick={onResetPosition}
                    disabled={cameraAutomationActive}
                    title="Klibi tuvalin merkezine getir"
                >Merkeze al</button>
            </div>

            <div class="row">
                <div class="control-group half">
                    <label for="pos-x">X konumu</label>
                    <input
                        id="pos-x"
                        type="number"
                        step="1"
                        value={transform.x}
                        disabled={cameraAutomationActive}
                        oninput={(event) => handleChange("x", event)}
                    />
                </div>
                <div class="control-group half">
                    <label for="pos-y">Y konumu</label>
                    <input
                        id="pos-y"
                        type="number"
                        step="1"
                        value={transform.y}
                        disabled={cameraAutomationActive}
                        oninput={(event) => handleChange("y", event)}
                    />
                </div>
            </div>
        </section>

        <div class="separator" aria-hidden="true"></div>

        <section class="section" aria-labelledby="effects-section-title">
            <h4 id="effects-section-title" class="section-title">Efektler</h4>
            
            <div class="control-group">
                <label for="effect-select">Aktif Efekt</label>
                <select
                    id="effect-select"
                    value={(transform.shake ?? 0) > 0 ? "shake" : "none"}
                    onchange={(event) => {
                        const val = event.currentTarget.value;
                        if (val === "shake") {
                            if (!(transform.shake && transform.shake > 0)) {
                                onUpdate("shake", 20);
                            }
                        } else {
                            onUpdate("shake", 0);
                        }
                    }}
                >
                    <option value="none">Yok</option>
                    <option value="shake">Kamera Sarsıntısı (Titreme)</option>
                </select>
            </div>

            {#if (transform.shake ?? 0) > 0}
                <div class="control-group">
                    <label for="shake-intensity">Sarsıntı Şiddeti</label>
                    <div class="control range-control">
                        <input
                            id="shake-intensity"
                            type="range"
                            min="1"
                            max="100"
                            step="1"
                            value={transform.shake ?? 20}
                            aria-valuetext={`${transform.shake ?? 20}%`}
                            oninput={(event) => handleChange("shake", event)}
                        />
                        <output class="value" for="shake-intensity"
                            >{transform.shake ?? 20}%</output
                        >
                    </div>
                </div>
            {/if}
        </section>
    {/if}

    {#if speed !== null && onSpeedChange}
        {#if hasVisualControls || clipKind === "text"}<div class="separator" aria-hidden="true"></div>{/if}

        <section class="section" aria-labelledby="clip-section-title">
            <h4 id="clip-section-title" class="section-title section-title-spaced">Klip</h4>
            <div class="row">
                <div class="control-group half">
                    <label for="clip-speed">Hız</label>
                    <input
                        id="clip-speed"
                        type="number"
                        min="0.05"
                        max="16"
                        step="0.05"
                        value={speed}
                        title="Oynatma hızı (1 = normal)"
                        onchange={handleSpeedChange}
                    />
                </div>
            </div>
        </section>
    {/if}

    {#if hasAudioControls}
        <div class="separator" aria-hidden="true"></div>

        <section class="section" aria-labelledby="audio-section-title">
            <h4 id="audio-section-title" class="section-title section-title-spaced">Ses</h4>
            <div class="control-group">
                <label for="volume">Klip taban sesi</label>
                <div class="control range-control">
                    <input
                        id="volume"
                        type="range"
                        min={MIN_EDITABLE_GAIN_DB}
                        max={MAX_EDITABLE_GAIN_DB}
                        step="0.1"
                        value={gainDbSliderValue(volume)}
                        aria-valuetext={formatGainReadout(volume)}
                        title="Klip kazancı (dB). 0 dB, kaynak klibin değiştirilmemiş seviyesidir."
                        oninput={handleVolumeDbChange}
                    />
                    <output class="value" for="volume"
                    >{formatGainReadout(volume)}</output
                    >
                </div>
            </div>
            <p class="hint">0 dB klibin orijinal seviyesidir; bu değer ölçülen dBFS/LUFS değil, eklenen kazançtır.</p>
            <div class="noise-reduction-panel compact" class:active={Boolean(noiseReduction)}>
                <div class="noise-reduction-heading">
                    <div>
                        <span class="ai-chip">AI · HIZLI</span>
                        <strong>Gürültü azalt</strong>
                        <small>Fan, motor, rüzgâr, dip ses ve uğultuyu tüm klipte otomatik azaltır.</small>
                    </div>
                    <button
                        type="button"
                        class="noise-toggle"
                        class:active={Boolean(noiseReduction)}
                        aria-pressed={Boolean(noiseReduction)}
                        disabled={noiseControlsDisabled || !onNoiseReductionChange}
                        onclick={toggleNoiseReduction}
                    >{noiseReductionBusy
                        ? "İşleniyor…"
                        : noiseReduction
                          ? "Aktif"
                          : "Aç"}</button>
                </div>
            </div>
            {#if noiseReductionMessage}
                <div class="noise-reduction-status" class:error={noiseReductionError} role={noiseReductionError ? "alert" : "status"}>
                    <span>{noiseReductionMessage}</span>
                    {#if noiseReductionError && onNoiseReductionRetry}
                        <button type="button" onclick={onNoiseReductionRetry} disabled={noiseReductionBusy}>Yeniden dene</button>
                    {/if}
                </div>
            {/if}

            <div class="studio-voice-panel" class:active={studioVoiceActive}>
                <div class="studio-voice-heading">
                    <div>
                        <span class="ai-chip studio">AI · STÜDYO</span>
                        <strong>Sesi geliştir</strong>
                        <small>Konuşmayı öne çıkarır; boğukluğu önlemek için doğal sesi koruyup tonu ve dinamikleri dengeler.</small>
                    </div>
                    <button
                        type="button"
                        class="studio-voice-toggle"
                        class:active={studioVoiceActive}
                        aria-pressed={studioVoiceActive}
                        disabled={noiseReductionBusy || mossFormerBusy || !onMossFormerPrepare}
                        onclick={toggleStudioVoice}
                        title="MossFormer2 + DeepFilterNet3 + doğal ses karışımı + stüdyo tonlaması"
                    >{mossFormerBusy
                        ? mossFormerProgressPercent === null
                            ? "İşleniyor"
                            : `%${mossFormerProgressPercent}`
                        : studioVoiceActive
                          ? "Aktif"
                          : mossFormerAvailable
                            ? "Kullan"
                            : "Geliştir"}</button>
                </div>

                <div class="studio-voice-chain" aria-label="Stüdyo ses geliştirme zinciri">
                    <span>AI ayırma</span><i>→</i><span>Doğal ton</span><i>→</i><span>Dinamik</span><i>→</i><span>Limiter</span>
                </div>
                <small class="studio-voice-honesty">
                    En iyi sonuç tek konuşmacıda alınır. Arka plandaki başka konuşmaları ana sesten kusursuz ayırmak her kayıtta mümkün değildir.
                </small>

                {#if mossFormerBusy}
                    <div class="hybrid-ai-progress" role="status" aria-live="polite">
                        <div class="hybrid-ai-progress-heading">
                            <span>
                                <b>Stüdyo sesi hazırlanıyor</b>
                                <small>MossFormer2 + DeepFilterNet3 + mastering</small>
                            </span>
                            <strong>{mossFormerProgressPercent === null
                                ? "İşleniyor"
                                : `%${mossFormerProgressPercent}`}</strong>
                        </div>
                        <div
                            class="hybrid-ai-progress-track"
                            role="progressbar"
                            aria-label="Stüdyo ses geliştirme ilerlemesi"
                            aria-valuemin="0"
                            aria-valuemax="100"
                            aria-valuenow={mossFormerProgressPercent ?? undefined}
                            aria-valuetext={mossFormerProgressPercent === null
                                ? "İlerleme hesaplanıyor"
                                : `%${mossFormerProgressPercent} tamamlandı`}
                        >
                            {#if mossFormerProgressPercent === null}
                                <span class="indeterminate"></span>
                            {:else}
                                <span style:width={`${mossFormerProgressPercent}%`}></span>
                            {/if}
                        </div>
                        <div class="hybrid-ai-progress-details">
                            <span>{audioEnhancementPhaseLabel(mossFormerProgress?.phase)}</span>
                            {#if mossFormerPhaseProgressPercent !== null}
                                <span>Aşama %{mossFormerPhaseProgressPercent}</span>
                            {/if}
                            {#if mossFormerProgressTime}
                                <span>{mossFormerProgressTime}</span>
                            {/if}
                        </div>
                        <small>{mossFormerProgress?.message ?? mossFormerMessage ?? "Stüdyo sesi hazırlanıyor…"}</small>
                    </div>
                {:else if mossFormerAvailable || mossFormerError}
                    <small class="mossformer-test-status" class:error={mossFormerError}>{mossFormerMessage}</small>
                {/if}

                {#if (noiseComparisonAvailable || mossFormerAvailable) && onNoiseAuditionModeChange}
                    <div class="noise-ab-card">
                        <div class="noise-ab-heading">
                            <span><b>A/B/C Karşılaştır</b><small>Aynı kare ve oynatma konumunda dinle</small></span>
                            <em
                                class:original={noiseAuditionMode === "original"}
                                class:mossformer={noiseAuditionMode === "mossformer"}
                            >
                                {noiseAuditionMode === "original"
                                    ? "A çalıyor"
                                    : noiseAuditionMode === "mossformer"
                                      ? "C çalıyor"
                                      : "B çalıyor"}
                            </em>
                        </div>
                        <div class="noise-ab-switch" role="group" aria-label="Orijinal, gürültüsü azaltılmış ve stüdyo sesi karşılaştırması">
                            <button
                                type="button"
                                class:active={noiseAuditionMode === "original"}
                                aria-pressed={noiseAuditionMode === "original"}
                                onclick={() => onNoiseAuditionModeChange?.("original")}
                            ><i>A</i><span>Orijinal</span></button>
                            <button
                                type="button"
                                class:active={noiseAuditionMode === "cleaned"}
                                aria-pressed={noiseAuditionMode === "cleaned"}
                                disabled={!noiseComparisonAvailable}
                                onclick={() => onNoiseAuditionModeChange?.("cleaned")}
                            ><i>B</i><span>Gürültü azalt</span></button>
                            <button
                                type="button"
                                class:active={noiseAuditionMode === "mossformer"}
                                aria-pressed={noiseAuditionMode === "mossformer"}
                                disabled={mossFormerBusy || (!mossFormerAvailable && !onMossFormerPrepare)}
                                onclick={() =>
                                    mossFormerAvailable
                                        ? onNoiseAuditionModeChange?.("mossformer")
                                        : onMossFormerPrepare?.()}
                                title="Stüdyo kalitesi · MossFormer2 + DeepFilterNet3 · cihazda çalışır"
                            ><i>C</i><span>{mossFormerAvailable ? "Stüdyo" : "Stüdyo hazırla"}</span></button>
                        </div>
                        <small class="noise-ab-export">Dışa aktarımda seçili sürüm kullanılır · A yalnız karşılaştırma içindir</small>
                    </div>
                {/if}
            </div>
            <div class="voice-rider-panel" class:active={Boolean(voiceRider)}>
                <div class="voice-rider-heading">
                    <span class="ai-chip">AI · CİHAZDA</span>
                    <strong>AI Voice Rider</strong>
                    <small>Silero Neural VAD konuşmayı bulur; bağırmayı kısıp kısık sesi dengeler.</small>
                </div>
                <div class="voice-rider-live">
                    <span>O an <b>{formatSignedDb(voiceRiderCurrentGain)}</b></span>
                    <span>Efektif <b>%{Math.round(effectiveVolume * 100)}</b></span>
                </div>
                {#if voiceRider}
                    <div class="voice-rider-stats">
                        <span>Konuşma %{Math.round(voiceRider.speechCoverage * 100)}</span>
                        <span>{voiceRider.strongestCutDb.toFixed(1)} dB kesme</span>
                        <span>+{voiceRider.strongestBoostDb.toFixed(1)} dB yükseltme</span>
                    </div>
                {/if}
                <div class="voice-rider-actions">
                    <button
                        type="button"
                        class="voice-rider-button"
                        disabled={disabled || !onVoiceRider || voiceRiderBusy || audioNormalizeBusy || noiseReductionBusy}
                        onclick={() => onVoiceRider?.()}
                    >{voiceRiderBusy
                        ? "AI analiz ediyor…"
                        : voiceRider
                          ? "Yeniden analiz et"
                          : "AI dengele"}</button>
                    {#if voiceRider}
                        <button
                            type="button"
                            class="voice-rider-remove"
                            disabled={disabled || voiceRiderBusy || !onVoiceRiderRemove}
                            onclick={() => onVoiceRiderRemove?.()}
                        >Kaldır</button>
                    {/if}
                </div>
            </div>
            {#if voiceRiderMessage}
                <p
                    class="voice-rider-message"
                    class:error={voiceRiderError}
                    role={voiceRiderError ? "alert" : "status"}
                >{voiceRiderMessage}</p>
            {/if}
            <div class="normalize-panel">
                <div>
                    <strong>Akıllı Normalize</strong>
                    <small>Voice hedefi · −16 LUFS · tepe korumalı</small>
                </div>
                <button
                    type="button"
                    class="normalize-button"
                    disabled={disabled || !onNormalizeAudio || audioNormalizeBusy || voiceRiderBusy || noiseReductionBusy || Boolean(voiceRider)}
                    title={voiceRider
                        ? "AI Voice Rider aktifken ayrıca klip normalizasyonu uygulanmaz"
                        : "Klibin tamamını −16 LUFS hedefine getir"}
                    onclick={() => onNormalizeAudio?.()}
                >{audioNormalizeBusy ? "Ses analiz ediliyor…" : "Akıllı normalize"}</button>
            </div>
            {#if audioNormalizeMessage}
                <p
                    class="normalize-message"
                    class:error={audioNormalizeError}
                    role={audioNormalizeError ? "alert" : "status"}
                >{audioNormalizeMessage}</p>
            {/if}
            {#if trackMix && onTrackMixChange}
                <div class="row">
                    <div class="control-group half">
                        <label for="track-gain">Kanal kazancı</label>
                        <input
                            id="track-gain"
                            type="number"
                            min={MIN_EDITABLE_GAIN_DB}
                            max={MAX_EDITABLE_GAIN_DB}
                            step="0.1"
                            value={gainDbSliderValue(trackMix.gain)}
                            aria-valuetext={formatGainReadout(trackMix.gain)}
                            title="Kanal kazancı (dB): kanaldaki tüm klipleri birlikte yükseltir veya kısar. 0 dB değiştirilmemiş seviyedir."
                            onchange={handleTrackGainDbChange}
                        />
                        <small>{formatGainReadout(trackMix.gain)}</small>
                    </div>
                    <div class="control-group half">
                        <label for="track-pan">Pan</label>
                        <input
                            id="track-pan"
                            type="number"
                            min="-1"
                            max="1"
                            step="0.05"
                            value={trackMix.pan}
                            title="Kanal pan: −1 sol, 0 orta, 1 sağ"
                            onchange={(event) => handleTrackMixChange("pan", event)}
                        />
                    </div>
                </div>
            {/if}
        </section>
    {/if}

    {#if audioRegion && hasAudioControls}
        <div class="separator" aria-hidden="true"></div>

        <section class="section" aria-labelledby="region-section-title">
            <h4 id="region-section-title" class="section-title section-title-spaced">Ses bölgesi</h4>
            {#if audioRegion.selection}
                <div class="region-editor">
                    <div class="region-range">
                        <span>Kısılacak aralık</span>
                        <strong
                            >{formatTime(audioRegion.selection.start)} –
                            {formatTime(audioRegion.selection.end)}</strong
                        >
                    </div>
                    <div class="control-group">
                        <label for="region-volume">
                            Bölge sesi
                        </label>
                        <div class="control range-control">
                            <input
                                id="region-volume"
                                type="range"
                                min="0"
                                max={Math.max(0.01, volume)}
                                step="0.01"
                                value={audioRegion.selection.volume}
                                aria-valuetext={`${Math.round((audioRegion.selection.volume / Math.max(volume, 0.0001)) * 100)}%`}
                                oninput={handleRegionVolumeInput}
                            />
                            <output class="value" for="region-volume"
                                >{formatRelativeGainReadout(audioRegion.selection.volume, volume)}</output
                            >
                        </div>
                    </div>
                    <p class="hint">Başlangıç ve bitişe kısa yumuşak geçiş otomatik eklenir.</p>
                    <div class="region-actions">
                        <button type="button" onclick={onAudioRegionCancel}>İptal</button>
                        <button type="button" class="danger" onclick={onAudioRegionRemove}>Kaldır</button>
                        <button
                            type="button"
                            class="primary"
                            disabled={audioRegion.selection.volume >= volume - 0.01}
                            title={audioRegion.selection.volume >= volume - 0.01
                                ? "Bölge sesi klibin genel sesinden daha düşük olmalı"
                                : "Seçilen ses seviyesini bu bölgeye uygula"}
                            onclick={onAudioRegionApply}>Sesi uygula</button
                        >
                    </div>
                </div>
            {:else}
                <button
                    type="button"
                    class="region-toggle"
                    class:active={audioRegion.selectMode}
                    aria-pressed={audioRegion.selectMode}
                    onclick={onAudioRegionToggle}
                >{audioRegion.selectMode
                        ? "Seçimi iptal et"
                        : "Dalga formunda bölge seç"}</button
                >
                <p class="hint">
                    Düğmeye basın, ardından zaman çizelgesindeki klibin ses
                    şeridinde kısılacak aralığı sürükleyin.
                </p>
                {#if audioRegion.quietRegions.length > 0}
                    <ul class="region-list">
                        {#each audioRegion.quietRegions as region (region.start)}
                            <li>
                                <button
                                    type="button"
                                    onclick={() => onQuietRegionOpen?.(region)}
                                >
                                    {formatTime(region.start)} – {formatTime(region.end)}
                                    <span>{formatRelativeGainReadout(region.volume, volume)}</span>
                                </button>
                            </li>
                        {/each}
                    </ul>
                {/if}
            {/if}
        </section>
    {/if}

    {#if onAddKeyframe}
        <div class="separator" aria-hidden="true"></div>

        <section class="section" aria-labelledby="keyframe-section-title">
            <h4 id="keyframe-section-title" class="section-title section-title-spaced">Anahtar kareler</h4>
            <div class="keyframe-buttons">
                {#if hasVisualControls}
                    <button type="button" onclick={() => onAddKeyframe("opacity")}
                        >◇ Opaklık</button
                    >
                {/if}
                {#if hasAudioControls}
                    <button type="button" onclick={() => onAddKeyframe("volume")}
                        >◇ Ses</button
                    >
                {/if}
            </div>
            <p class="hint">Oynatma kafasının bulunduğu ana mevcut değerle anahtar kare ekler.</p>
        </section>
    {/if}
    </fieldset>
</div>

<style>
    .inspector {
        width: 100%;
        height: 100%;
        padding: 14px;
        overflow-y: auto;
        box-sizing: border-box;
        background: #0f0f0f;
        color: #e0e0e0;
        font-family: "Inter", sans-serif;
    }

    .header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        margin-bottom: 16px;
    }

    .header h3 {
        margin: 0;
        color: #f1f3f2;
        font-size: 13px;
        font-weight: 650;
        letter-spacing: 0.06em;
        text-transform: uppercase;
    }

    .delete-button {
        min-height: 30px;
        flex: 0 0 auto;
        padding: 5px 11px;
        border: 1px solid #613a36;
        border-radius: 5px;
        color: #dca49d;
        background: #251615;
        font: 650 10px/1 "Inter", sans-serif;
        cursor: pointer;
        transition: border-color 0.15s, color 0.15s, background 0.15s;
    }

    .delete-button:hover:not(:disabled),
    .delete-button:focus-visible {
        border-color: #ae5a51;
        color: #ffd0ca;
        background: #341a18;
    }

    .inspector-fields {
        min-width: 0;
        margin: 0;
        padding: 0;
        border: 0;
    }

    .locked-note {
        margin: -4px 0 14px;
        padding: 8px 9px;
        border: 1px solid #4b4430;
        border-radius: 5px;
        color: #d8c68f;
        background: #211e14;
        font-size: 10px;
        line-height: 1.35;
    }

    .separated-note {
        margin: -4px 0 14px;
        padding: 8px 9px;
        border: 1px solid #31594e;
        border-radius: 5px;
        color: #83e3c5;
        background: #15231f;
        font-size: 10px;
        line-height: 1.35;
    }

    .camera-automation-note {
        margin: 0 0 12px;
        padding: 8px 9px;
        color: #9edbc6;
        background: rgba(57, 133, 106, .12);
        border: 1px solid rgba(101, 215, 178, .25);
        border-radius: 5px;
        font-size: 9px;
        line-height: 1.4;
    }

    .section {
        margin: 0;
    }

    .section-title {
        margin: 0;
        color: #8f9793;
        font-size: 11px;
        font-weight: 700;
        letter-spacing: 0.08em;
        text-transform: uppercase;
    }

    .section-title-spaced {
        margin-bottom: 12px;
    }

    .section-heading,
    .position-heading {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        margin-bottom: 12px;
    }

    .position-heading {
        margin-top: 2px;
        margin-bottom: 8px;
        color: #8f9793;
        font-size: 10px;
        font-weight: 700;
        letter-spacing: 0.05em;
        text-transform: uppercase;
    }

    .reset-button {
        min-height: 32px;
        flex: 0 0 auto;
        padding: 6px 9px;
        border: 1px solid #343a37;
        border-radius: 5px;
        background: #171918;
        color: #bdc5c1;
        font: 650 10px/1 "Inter", sans-serif;
        cursor: pointer;
        touch-action: manipulation;
        transition: border-color 0.15s, color 0.15s, background 0.15s;
    }

    .reset-button.compact {
        min-height: 30px;
        padding: 5px 8px;
        letter-spacing: 0;
        text-transform: none;
    }

    .reset-button:hover {
        border-color: #5fd0ae;
        background: #19221f;
        color: #73e1bf;
    }

    .reset-button:active {
        transform: translateY(1px);
    }

    .normalize-panel {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 10px;
        margin: 2px 0 13px;
        padding: 10px;
        border: 1px solid #2c4b42;
        border-radius: 7px;
        background: linear-gradient(135deg, rgba(72, 173, 140, 0.1), #151817);
    }

    .noise-reduction-panel {
        display: grid;
        gap: 10px;
        margin: 3px 0 13px;
        padding: 10px;
        border: 1px solid #4b3e68;
        border-radius: 8px;
        background:
            radial-gradient(circle at 92% 0%, rgba(155, 101, 255, 0.17), transparent 45%),
            linear-gradient(145deg, #191527, #15181b 72%);
        box-shadow: inset 0 1px rgba(255, 255, 255, 0.025);
    }

    .noise-reduction-panel.active {
        border-color: #8c6bd2;
        box-shadow: 0 0 0 1px rgba(154, 111, 241, 0.1), inset 0 1px rgba(255, 255, 255, 0.04);
    }

    .noise-reduction-panel.compact {
        gap: 0;
        margin-bottom: 9px;
        padding: 8px 9px;
        border-color: #343945;
        background: linear-gradient(145deg, #181a1e, #141619);
        box-shadow: none;
    }

    .noise-reduction-panel.compact.active {
        border-color: #76609e;
        background: linear-gradient(145deg, #1d1925, #16171b);
        box-shadow: inset 2px 0 #9b7bd6;
    }

    .noise-reduction-heading {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: 9px;
    }

    .noise-reduction-heading strong,
    .noise-reduction-heading small {
        display: block;
    }

    .noise-reduction-heading strong {
        margin-top: 6px;
        color: #eee7ff;
        font-size: 11px;
    }

    .noise-reduction-panel.compact .noise-reduction-heading strong {
        margin-top: 4px;
        font-size: 10px;
    }

    .noise-reduction-panel.compact .noise-reduction-heading small {
        max-width: 205px;
        margin-top: 2px;
        color: #858995;
        line-height: 1.35;
    }

    .noise-reduction-heading small {
        margin-top: 4px;
        color: #948ca7;
        font-size: 8px;
        line-height: 1.45;
    }

    .noise-toggle {
        min-width: 42px;
        min-height: 28px;
        border: 1px solid #615676;
        border-radius: 999px;
        color: #aaa0ba;
        background: #211c2a;
        font: 700 8px/1 "Inter", sans-serif;
        cursor: pointer;
    }

    .noise-toggle.active {
        border-color: #a987f0;
        color: #f2eaff;
        background: #533d7d;
    }

    .noise-toggle:disabled {
        cursor: wait;
        opacity: 0.5;
    }

    .studio-voice-panel {
        display: grid;
        gap: 9px;
        margin: 3px 0 13px;
        padding: 11px;
        border: 1px solid #27566a;
        border-radius: 9px;
        background:
            radial-gradient(circle at 92% 0%, rgba(47, 189, 224, 0.17), transparent 43%),
            linear-gradient(145deg, #10232b, #14191d 72%);
        box-shadow: inset 0 1px rgba(255, 255, 255, 0.035);
    }

    .studio-voice-panel.active {
        border-color: #52bdd7;
        box-shadow: 0 0 0 1px rgba(75, 203, 230, 0.09), inset 0 1px rgba(255, 255, 255, 0.05);
    }

    .studio-voice-heading {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: 9px;
    }

    .studio-voice-heading strong,
    .studio-voice-heading small {
        display: block;
    }

    .studio-voice-heading strong {
        margin-top: 6px;
        color: #e3faff;
        font-size: 12px;
    }

    .studio-voice-heading small,
    .studio-voice-honesty {
        margin-top: 4px;
        color: #82a3ac;
        font-size: 8px;
        line-height: 1.45;
    }

    .studio-voice-toggle {
        flex: 0 0 auto;
        min-width: 54px;
        min-height: 31px;
        padding: 0 9px;
        border: 1px solid #397a8d;
        border-radius: 999px;
        color: #b7e1e9;
        background: #17333d;
        font: 800 8px/1 "Inter", sans-serif;
        cursor: pointer;
    }

    .studio-voice-toggle.active {
        border-color: #79d7e9;
        color: #effdff;
        background: linear-gradient(180deg, #2389a1, #176274);
        box-shadow: 0 2px 9px rgba(10, 61, 73, 0.38);
    }

    .studio-voice-toggle:disabled {
        cursor: wait;
        opacity: 0.58;
    }

    .studio-voice-chain {
        display: flex;
        align-items: center;
        justify-content: center;
        gap: 4px;
        padding: 6px;
        border: 1px solid rgba(73, 165, 188, 0.24);
        border-radius: 6px;
        color: #9bd3df;
        background: rgba(6, 29, 35, 0.54);
        font-size: 7px;
    }

    .studio-voice-chain i {
        color: #4f8793;
        font-style: normal;
    }

    .studio-voice-honesty {
        margin: -2px 0 0;
    }

    .noise-ab-card {
        display: grid;
        gap: 7px;
        padding: 9px;
        border: 1px solid rgba(170, 132, 245, 0.5);
        border-radius: 8px;
        background:
            linear-gradient(135deg, rgba(98, 61, 151, 0.28), rgba(23, 18, 33, 0.92));
        box-shadow: inset 0 1px rgba(255, 255, 255, 0.04);
    }

    .noise-ab-heading {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
    }

    .noise-ab-heading > span {
        display: grid;
        gap: 2px;
    }

    .noise-ab-heading b {
        color: #f0e8ff;
        font-size: 9px;
    }

    .noise-ab-heading small,
    .noise-ab-export {
        color: #968ca3;
        font-size: 7px;
    }

    .noise-ab-heading em {
        padding: 4px 6px;
        border-radius: 999px;
        color: #d9c8ff;
        background: rgba(124, 83, 199, 0.34);
        font: 700 7px/1 "Inter", sans-serif;
        font-style: normal;
        white-space: nowrap;
    }

    .noise-ab-heading em.original {
        color: #ffd9a2;
        background: rgba(176, 111, 36, 0.28);
    }

    .noise-ab-heading em.mossformer {
        color: #bfefff;
        background: rgba(34, 130, 168, 0.3);
    }

    .noise-ab-switch {
        display: grid;
        grid-template-columns: repeat(3, minmax(0, 1fr));
        gap: 5px;
        padding: 3px;
        border-radius: 7px;
        background: rgba(5, 4, 9, 0.48);
    }

    .noise-ab-switch button {
        display: flex;
        align-items: center;
        justify-content: center;
        gap: 5px;
        min-height: 30px;
        border: 1px solid transparent;
        border-radius: 5px;
        color: #8f8799;
        background: transparent;
        font: 700 8px/1 "Inter", sans-serif;
        cursor: pointer;
    }

    .noise-ab-switch button:hover {
        color: #ddd3e8;
        background: rgba(255, 255, 255, 0.04);
    }

    .noise-ab-switch button.active {
        border-color: #8867ce;
        color: #f4eeff;
        background: linear-gradient(180deg, #654b91, #483568);
        box-shadow: 0 2px 8px rgba(17, 10, 29, 0.38);
    }

    .noise-ab-switch button:first-child.active {
        border-color: #9e713f;
        background: linear-gradient(180deg, #75532f, #50381f);
    }

    .noise-ab-switch button:last-child.active {
        border-color: #4ca5c6;
        background: linear-gradient(180deg, #326f86, #234b5d);
    }

    .noise-ab-switch button:disabled {
        cursor: wait;
        opacity: 0.55;
    }

    .noise-ab-switch i {
        display: grid;
        place-items: center;
        width: 16px;
        height: 16px;
        border-radius: 4px;
        color: currentColor;
        background: rgba(255, 255, 255, 0.08);
        font-size: 8px;
        font-style: normal;
    }

    .noise-ab-export {
        text-align: center;
    }

    .mossformer-test-status {
        color: #9ccbdd;
        font-size: 7px;
        line-height: 1.45;
        text-align: center;
    }

    .mossformer-test-status.error {
        color: #e4aaa2;
    }

    .hybrid-ai-progress {
        display: grid;
        gap: 6px;
        padding: 8px;
        border: 1px solid rgba(79, 178, 209, 0.42);
        border-radius: 7px;
        color: #a7ddea;
        background: rgba(12, 35, 43, 0.72);
    }

    .hybrid-ai-progress-heading {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
    }

    .hybrid-ai-progress-heading > span {
        display: grid;
        gap: 1px;
    }

    .hybrid-ai-progress-heading b {
        color: #e3faff;
        font-size: 9px;
    }

    .hybrid-ai-progress-heading small,
    .hybrid-ai-progress > small {
        color: #88b5c0;
        font-size: 7px;
        line-height: 1.4;
    }

    .hybrid-ai-progress-heading strong {
        min-width: 34px;
        color: #c9f6ff;
        font: 800 10px/1 "Inter", sans-serif;
        text-align: right;
    }

    .hybrid-ai-progress-track {
        position: relative;
        height: 6px;
        overflow: hidden;
        border-radius: 999px;
        background: rgba(3, 13, 17, 0.72);
        box-shadow: inset 0 0 0 1px rgba(126, 222, 243, 0.1);
    }

    .hybrid-ai-progress-track > span {
        display: block;
        height: 100%;
        border-radius: inherit;
        background: linear-gradient(90deg, #318fb1, #78e0ee);
        transition: width 180ms ease-out;
    }

    .hybrid-ai-progress-track > span.indeterminate {
        width: 38%;
        animation: hybrid-progress-slide 1.15s ease-in-out infinite;
    }

    .hybrid-ai-progress-details {
        display: flex;
        flex-wrap: wrap;
        justify-content: space-between;
        gap: 3px 8px;
        color: #76aab6;
        font-size: 7px;
    }

    .hybrid-ai-progress-details span:first-child {
        color: #b8e8f2;
        font-weight: 700;
    }

    @keyframes hybrid-progress-slide {
        from { transform: translateX(-110%); }
        to { transform: translateX(285%); }
    }

    .noise-reduction-status {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 7px;
        margin: -5px 0 13px;
        padding: 7px 8px;
        border: 1px solid #51406f;
        border-radius: 5px;
        color: #c5acef;
        background: #20182c;
        font-size: 9px;
        line-height: 1.4;
    }

    .noise-reduction-status.error {
        color: #e4aaa2;
        border-color: #693d37;
        background: #2b1816;
    }

    .noise-reduction-status button {
        flex: 0 0 auto;
        border: 1px solid currentColor;
        border-radius: 4px;
        color: inherit;
        background: transparent;
        font-size: 8px;
        cursor: pointer;
    }

    .voice-rider-panel {
        display: grid;
        gap: 9px;
        margin: 3px 0 13px;
        padding: 10px;
        border: 1px solid #315964;
        border-radius: 8px;
        background:
            radial-gradient(circle at 88% 0%, rgba(69, 211, 242, 0.15), transparent 43%),
            linear-gradient(145deg, #102229, #14191b 72%);
        box-shadow: inset 0 1px rgba(255, 255, 255, 0.025);
    }

    .voice-rider-panel.active {
        border-color: #55b9cb;
        box-shadow: 0 0 0 1px rgba(77, 207, 234, 0.08), inset 0 1px rgba(255, 255, 255, 0.04);
    }

    .voice-rider-heading strong,
    .voice-rider-heading small {
        display: block;
    }

    .voice-rider-heading strong {
        margin-top: 6px;
        color: #def9ff;
        font-size: 11px;
    }

    .voice-rider-heading small {
        margin-top: 4px;
        color: #7e9aa0;
        font-size: 8px;
        line-height: 1.45;
    }

    .ai-chip {
        display: inline-flex;
        padding: 2px 5px;
        border: 1px solid rgba(116, 233, 255, 0.52);
        border-radius: 4px;
        color: #8ceeff;
        background: rgba(21, 69, 79, 0.48);
        font: 750 7px/1.25 "Inter", sans-serif;
        letter-spacing: 0.09em;
    }

    .voice-rider-live,
    .voice-rider-stats {
        display: flex;
        flex-wrap: wrap;
        gap: 5px;
    }

    .voice-rider-live span,
    .voice-rider-stats span {
        padding: 4px 6px;
        border: 1px solid rgba(103, 179, 193, 0.2);
        border-radius: 4px;
        color: #89a4a9;
        background: rgba(5, 18, 22, 0.5);
        font-size: 8px;
    }

    .voice-rider-live b {
        color: #d7f8ff;
        font-weight: 700;
    }

    .voice-rider-stats span {
        color: #7fcbd8;
    }

    .voice-rider-actions {
        display: flex;
        gap: 6px;
    }

    .voice-rider-button,
    .voice-rider-remove {
        min-height: 31px;
        border-radius: 5px;
        font: 700 9px/1.2 "Inter", sans-serif;
        cursor: pointer;
    }

    .voice-rider-button {
        flex: 1;
        border: 1px solid #4ca8ba;
        color: #bceff8;
        background: #173a43;
    }

    .voice-rider-button:hover:not(:disabled) {
        border-color: #76e8fb;
        color: #effdff;
        background: #1b4b56;
    }

    .voice-rider-remove {
        padding: 0 8px;
        border: 1px solid #3e555a;
        color: #93a5a8;
        background: #1b2426;
    }

    .voice-rider-button:disabled,
    .voice-rider-remove:disabled {
        cursor: wait;
        opacity: 0.55;
    }

    .voice-rider-message {
        margin: -5px 0 13px;
        padding: 7px 8px;
        border: 1px solid #315e68;
        border-radius: 5px;
        color: #91dce9;
        background: #11282d;
        font-size: 9px;
        line-height: 1.4;
    }

    .voice-rider-message.error {
        color: #e4aaa2;
        border-color: #693d37;
        background: #2b1816;
    }

    .normalize-panel strong,
    .normalize-panel small {
        display: block;
    }

    .normalize-panel strong {
        color: #d7e8e2;
        font-size: 10px;
    }

    .normalize-panel small {
        margin-top: 4px;
        color: #77827e;
        font-size: 8px;
        line-height: 1.35;
    }

    .normalize-button {
        min-height: 31px;
        flex: 0 0 auto;
        padding: 6px 9px;
        border: 1px solid #408d76;
        border-radius: 5px;
        color: #a7f1d8;
        background: #173329;
        font: 650 9px/1.2 "Inter", sans-serif;
        cursor: pointer;
    }

    .normalize-button:hover:not(:disabled) {
        color: #dcfff3;
        border-color: #65d5b2;
        background: #1b4435;
    }

    .normalize-button:disabled {
        cursor: wait;
        opacity: 0.55;
    }

    .normalize-message {
        margin: -5px 0 13px;
        padding: 7px 8px;
        border: 1px solid #2f594c;
        border-radius: 5px;
        color: #94d9c1;
        background: #13241f;
        font-size: 9px;
        line-height: 1.4;
    }

    .normalize-message.error {
        color: #e4aaa2;
        border-color: #693d37;
        background: #2b1816;
    }

    .control-group {
        min-width: 0;
        margin-bottom: 13px;
    }

    .control-group > label,
    .control-label {
        display: block;
        margin-bottom: 6px;
        color: #a4aaa7;
        font-size: 11px;
        line-height: 1.25;
    }

    .content-control {
        margin-top: 12px;
    }

    .control {
        display: flex;
        align-items: center;
        gap: 10px;
    }

    .range-control input[type="range"] {
        min-width: 0;
        height: 32px;
        flex: 1;
        margin: 0;
        appearance: none;
        background: transparent;
        cursor: pointer;
        touch-action: pan-y;
    }

    .range-control input[type="range"]::-webkit-slider-runnable-track {
        height: 5px;
        border-radius: 999px;
        background: #343837;
    }

    .range-control input[type="range"]::-webkit-slider-thumb {
        width: 17px;
        height: 17px;
        margin-top: -6px;
        appearance: none;
        border: 2px solid #101211;
        border-radius: 50%;
        background: #69d9b5;
        box-shadow: 0 0 0 1px rgba(105, 217, 181, 0.28);
        cursor: grab;
    }

    .range-control input[type="range"]:active::-webkit-slider-thumb {
        cursor: grabbing;
        background: #82e8c7;
    }

    input[type="number"],
    select,
    textarea {
        width: 100%;
        min-width: 0;
        box-sizing: border-box;
        border: 1px solid #363a38;
        border-radius: 5px;
        background: #191a1a;
        color: #f0f2f1;
        font: 12px/1.4 "Inter", sans-serif;
    }

    input[type="number"],
    select {
        min-height: 36px;
        padding: 6px 8px;
    }

    textarea {
        min-height: 92px;
        padding: 9px 10px;
        resize: vertical;
        user-select: text;
    }

    textarea::placeholder {
        color: #666d69;
    }

    input:disabled,
    select:disabled,
    textarea:disabled,
    button:disabled {
        cursor: not-allowed;
        opacity: 0.48;
    }

    input:focus-visible,
    select:focus-visible,
    textarea:focus-visible,
    button:focus-visible {
        border-color: #69d9b5;
        outline: 2px solid rgba(105, 217, 181, 0.38);
        outline-offset: 2px;
    }

    .value {
        min-width: 46px;
        color: #c1c7c4;
        font: 11px/1 "JetBrains Mono", monospace;
        text-align: right;
    }

    .row {
        display: flex;
        gap: 10px;
    }

    .half {
        flex: 1 1 0;
    }

    .text-row,
    .color-row {
        align-items: flex-start;
    }

    .color-control {
        min-height: 36px;
        display: flex;
        align-items: center;
        gap: 7px;
        padding: 4px 7px 4px 5px;
        box-sizing: border-box;
        border: 1px solid #363a38;
        border-radius: 5px;
        background: #191a1a;
    }

    .color-control input[type="color"] {
        width: 30px;
        height: 26px;
        flex: 0 0 auto;
        padding: 0;
        border: 1px solid #454a47;
        border-radius: 4px;
        background: transparent;
        cursor: pointer;
    }

    .color-control span {
        min-width: 0;
        overflow: hidden;
        color: #b8bebb;
        font: 9px/1 "JetBrains Mono", monospace;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .transparency-toggle {
        min-height: 34px;
        display: flex;
        align-items: center;
        gap: 8px;
        margin: -3px 0 13px;
        color: #b0b6b3;
        font-size: 11px;
        cursor: pointer;
    }

    .transparency-toggle input {
        width: 17px;
        height: 17px;
        flex: 0 0 auto;
        margin: 0;
        accent-color: #69d9b5;
    }

    .alignment-buttons {
        display: grid;
        grid-template-columns: repeat(3, minmax(0, 1fr));
        gap: 5px;
    }

    .alignment-buttons button {
        min-height: 36px;
        padding: 6px 5px;
        border: 1px solid #343937;
        border-radius: 5px;
        background: #191b1a;
        color: #aeb5b1;
        font: 600 10px/1 "Inter", sans-serif;
        cursor: pointer;
        touch-action: manipulation;
    }

    .alignment-buttons button:hover:not(:disabled) {
        border-color: #52605b;
        color: #e6ebe8;
    }

    .alignment-buttons button.active {
        border-color: #65d7b2;
        background: #1b3029;
        color: #7ce4c2;
    }

    .separator {
        height: 1px;
        margin: 20px 0;
        background: #252827;
    }

    .hint {
        margin: 2px 0 0;
        color: #747d79;
        font-size: 10px;
        line-height: 1.45;
    }

    .region-editor {
        display: flex;
        flex-direction: column;
        gap: 10px;
        padding: 11px;
        border: 1px solid rgba(233, 184, 76, 0.46);
        border-radius: 6px;
        background:
            linear-gradient(135deg, rgba(151, 105, 20, 0.15), transparent 58%),
            #18150f;
    }

    .region-editor .control-group {
        margin-bottom: 0;
    }

    .region-range {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        color: #aeb6b2;
        font-size: 10px;
    }

    .region-range strong {
        color: #ffe3a4;
        font: 700 10px/1 "JetBrains Mono", monospace;
    }

    .region-actions {
        display: flex;
        align-items: center;
        gap: 6px;
    }

    .region-actions button,
    .region-toggle {
        min-height: 32px;
        padding: 5px 9px;
        border: 1px solid #343b38;
        border-radius: 5px;
        color: #aeb7b3;
        background: #181b1a;
        font: 600 10px/1 "Inter", sans-serif;
        cursor: pointer;
        touch-action: manipulation;
    }

    .region-actions button {
        flex: 1;
    }

    .region-toggle {
        width: 100%;
    }

    .region-actions button:hover:not(:disabled),
    .region-toggle:hover:not(:disabled),
    .region-toggle.active {
        color: #f7fffc;
        border-color: #58bfa2;
        background: #18342c;
    }

    .region-actions button.primary {
        color: #2a210d;
        border-color: #e1b04e;
        background: #e9bd62;
        font-weight: 750;
    }

    .region-actions button.primary:hover:not(:disabled) {
        color: #1d1709;
        border-color: #ffdc8c;
        background: #f5cd79;
    }

    .region-actions button.danger {
        color: #dca49d;
        border-color: #613a36;
        background: #251615;
    }

    .region-actions button.danger:hover:not(:disabled) {
        color: #ffd0ca;
        border-color: #ae5a51;
        background: #341a18;
    }

    .region-list {
        display: flex;
        flex-direction: column;
        gap: 5px;
        margin: 10px 0 0;
        padding: 0;
        list-style: none;
    }

    .region-list button {
        display: flex;
        width: 100%;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        min-height: 30px;
        padding: 5px 9px;
        border: 1px solid #3d4441;
        border-radius: 5px;
        color: #c6cfcb;
        background: #14201c;
        font: 600 10px/1 "JetBrains Mono", monospace;
        cursor: pointer;
    }

    .region-list button:hover:not(:disabled) {
        color: #fff9dd;
        border-color: rgba(255, 210, 104, 0.82);
        background: #221c0f;
    }

    .region-list button span {
        color: #ffe3a2;
    }

    .animation-row {
        display: grid;
        grid-template-columns: 38px minmax(0, 1fr) 64px;
        align-items: center;
        gap: 8px;
        margin-bottom: 10px;
    }

    .animation-label {
        color: #a4aaa7;
        font-size: 11px;
    }

    .subsection-title {
        margin: 16px 0 10px;
        padding-top: 12px;
        border-top: 1px solid #212423;
    }

    .preset-control {
        margin-top: 2px;
    }

    .preset-grid {
        display: grid;
        grid-template-columns: repeat(4, minmax(0, 1fr));
        gap: 5px;
    }

    .preset-chip {
        min-height: 30px;
        padding: 5px 4px;
        border: 1px solid #343937;
        border-radius: 5px;
        font: 700 10px/1 "Inter", sans-serif;
        cursor: pointer;
        touch-action: manipulation;
        transition: border-color 0.15s, transform 0.1s;
    }

    .preset-chip:hover:not(:disabled) {
        border-color: #5fd0ae;
        transform: translateY(-1px);
    }

    .preset-chip:active:not(:disabled) {
        transform: translateY(0);
    }

    .preset-chip:disabled {
        cursor: not-allowed;
        opacity: 0.48;
    }

    .keyframe-buttons {
        display: grid;
        grid-template-columns: repeat(auto-fit, minmax(0, 1fr));
        gap: 6px;
    }

    .keyframe-buttons button {
        min-height: 32px;
        padding: 6px 8px;
        border: 1px solid #343937;
        border-radius: 5px;
        background: #191b1a;
        color: #aeb5b1;
        font: 600 10px/1 "Inter", sans-serif;
        cursor: pointer;
        touch-action: manipulation;
    }

    .keyframe-buttons button:hover:not(:disabled) {
        border-color: #52605b;
        color: #e6ebe8;
    }

    .empty-state {
        margin: 12px 0 0;
        padding: 10px;
        border: 1px dashed #303331;
        border-radius: 5px;
        color: #858b88;
        font-size: 11px;
    }

    @media (max-width: 220px) {
        .row {
            flex-direction: column;
            gap: 0;
        }

        .half {
            width: 100%;
        }
    }
</style>
