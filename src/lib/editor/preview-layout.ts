export interface PreviewDisplaySize {
  width: number;
  height: number;
}

/**
 * Fits a project frame inside the visible preview stage. The calculation is
 * intentionally independent from the canvas bitmap's intrinsic dimensions so
 * large editor windows can upscale the preview while preserving its aspect.
 */
export function fitPreviewDisplaySize(
  containerWidth: number,
  containerHeight: number,
  contentWidth: number,
  contentHeight: number,
  totalInset = 0,
): PreviewDisplaySize {
  if (
    !Number.isFinite(containerWidth) ||
    !Number.isFinite(containerHeight) ||
    !Number.isFinite(contentWidth) ||
    !Number.isFinite(contentHeight) ||
    !Number.isFinite(totalInset) ||
    contentWidth <= 0 ||
    contentHeight <= 0
  ) {
    return { width: 0, height: 0 };
  }

  const availableWidth = Math.max(0, containerWidth - Math.max(0, totalInset));
  const availableHeight = Math.max(0, containerHeight - Math.max(0, totalInset));
  if (availableWidth === 0 || availableHeight === 0) {
    return { width: 0, height: 0 };
  }

  const scale = Math.min(
    availableWidth / contentWidth,
    availableHeight / contentHeight,
  );
  return {
    width: contentWidth * scale,
    height: contentHeight * scale,
  };
}
