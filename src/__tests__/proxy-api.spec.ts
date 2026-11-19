// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import { applyProxySettings, getProxySnapshot } from "../api/proxy";

describe("proxy API", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue({});
  });

  it("getProxySnapshot_should传目标服务_when提供macOS服务名", async () => {
    await getProxySnapshot("Ethernet");

    expect(invokeMock).toHaveBeenCalledWith("get_proxy_snapshot", {
      target: "Ethernet",
    });
  });

  it("getProxySnapshot_should省略目标参数_when未提供服务名", async () => {
    await getProxySnapshot();

    expect(invokeMock).toHaveBeenCalledWith("get_proxy_snapshot", {});
  });

  it("applyProxySettings_should使用命令请求包装", async () => {
    const request = { terminal: null, system: null };
    await applyProxySettings(request);

    expect(invokeMock).toHaveBeenCalledWith("apply_proxy_settings", {
      request,
    });
  });
});
