// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useGitProxy } from "../composables/useGitProxy";
const { read, apply } = vi.hoisted(() => ({ read: vi.fn(), apply: vi.fn() }));
vi.mock("../api/proxy", () => ({ getGitProxySnapshot: read, applyGitProxySettings: apply }));
const snapshot = {
  available: true,
  writable: true,
  mode: "custom" as const,
  proxy: "http://***:***@proxy.example:9000",
  revision: "v1",
  warnings: [],
};
beforeEach(() => {
  vi.restoreAllMocks();
  read.mockReset().mockResolvedValue(snapshot);
  apply.mockReset().mockResolvedValue({ status: "applied", message: "已保存", snapshot });
  vi.spyOn(window, "confirm").mockReturnValue(true);
});
describe("Git草稿", () => {
  it("未编辑脱敏地址不写回掩码", async () => {
    const state = useGitProxy();
    await state.reload();
    await state.save();
    expect(apply).toHaveBeenCalledWith({ mode: "custom", proxy: null, expectedRevision: "v1" });
  });
  it.each(["inherit", "disabled"] as const)("%s模式独立写入且无终端/系统字段", async (mode) => {
    const state = useGitProxy();
    await state.reload();
    state.setMode(mode);
    await state.save();
    expect(apply).toHaveBeenCalledWith({ mode, proxy: null, expectedRevision: "v1" });
  });
  it("冲突与部分写入保留草稿和旧revision，重读后恢复", async () => {
    const state = useGitProxy();
    await state.reload();
    state.setProxy("http://draft.example:8080");
    apply.mockResolvedValueOnce({ status: "partial", message: "写入后回读失败", snapshot: null });
    await state.save();
    expect(state.draft.proxy).toBe("http://draft.example:8080");
    expect(state.dirty.value).toBe(true);
    expect(state.snapshot.value?.revision).toBe("v1");
    await state.reload();
    expect(state.dirty.value).toBe(false);
  });
  it("取消重读零命令，不可写禁止保存", async () => {
    const state = useGitProxy();
    await state.reload();
    state.setProxy("draft");
    vi.mocked(window.confirm).mockReturnValue(false);
    await state.reload();
    expect(read).toHaveBeenCalledTimes(1);
    expect(apply).not.toHaveBeenCalled();
    vi.mocked(window.confirm).mockReturnValue(true);
    read.mockResolvedValueOnce({ ...snapshot, writable: false, warnings: ["多来源"] });
    await state.reload();
    await state.save();
    expect(apply).not.toHaveBeenCalled();
  });
  it("从继承切至指定模式必须提供完整新地址", async () => {
    read.mockResolvedValueOnce({ ...snapshot, mode: "inherit", proxy: null });
    const state = useGitProxy();
    await state.reload();
    state.setMode("custom");
    await state.save();
    expect(apply).not.toHaveBeenCalled();
    expect(state.state.saveError).toContain("完整");
  });
});
