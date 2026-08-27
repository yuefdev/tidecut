import { describe, expect, it } from "vitest";
import { normalizeProjectCover } from "./cover";

const cover = {
  imageDataUrl: "data:image/jpeg;base64,Y292ZXI=",
  timeMs: 1_234.7,
  aspect: "16:9",
  updatedAtMs: 9_876.2,
};

describe("project cover persistence contract", () => {
  it("keeps a valid compact image cover with stable frame metadata", () => {
    expect(normalizeProjectCover(cover)).toEqual({
      ...cover,
      timeMs: 1_235,
      updatedAtMs: 9_876,
    });
  });

  it("fails closed for non-image data and malformed cover metadata", () => {
    expect(normalizeProjectCover({ ...cover, imageDataUrl: "data:text/html;base64,PGgxPg==" })).toBeNull();
    expect(normalizeProjectCover({ ...cover, timeMs: -1 })).toBeNull();
    expect(normalizeProjectCover({ ...cover, aspect: "4:3" })).toBeNull();
  });
});
