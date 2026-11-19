// @vitest-environment jsdom
import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ApplyReport, ProxySnapshot, SectionResult } from "../generated/bindings";

const { getProxySnapshotMock, applyProxySettingsMock } = vi.hoisted(() => ({
  getProxySnapshotMock: vi.fn(),
  applyProxySettingsMock: vi.fn(),
}));

vi.mock("../api/proxy", () => ({
  getProxySnapshot: getProxySnapshotMock,
  applyProxySettings: applyProxySettingsMock,
}));

import App from "../App.vue";
import { useProxySettings } from "../composables/useProxySettings";

function makeSnapshot(platform: "macos" | "windows" = "macos"): ProxySnapshot {
  return {
    platform,
    terminal: {
      process: {
        httpProxy: "http://process:secret@127.0.0.1:8080",
        httpsProxy: null,
        allProxy: null,
        noProxy: "localhost",
      },
      persisted: {
        httpProxy: "http://alice:secret@127.0.0.1:8080",
        httpsProxy: "http://127.0.0.1:8443",
        allProxy: null,
        noProxy: "localhost,127.0.0.1",
      },
      persistenceLabel: "测试 Shell 配置区块",
      launchd:
        platform === "macos"
          ? {
              httpProxy: "http://launchd:secret@127.0.0.1:8080",
              httpsProxy: null,
              allProxy: null,
              noProxy: null,
            }
          : null,
      warnings: ["已有终端需重载配置"],
    },
    system: {
      enabled: true,
      http: { host: "127.0.0.1", port: 8080 },
      https: { host: "127.0.0.1", port: 8443 },
      socks: null,
      bypass: ["localhost", "127.0.0.1"],
      availableTargets: platform === "macos" ? ["Wi-Fi", "Ethernet"] : [],
      selectedTarget: platform === "macos" ? "Wi-Fi" : null,
      autoConfig: "https://pac:secret@example.test/proxy.pac",
      warnings: ["检测到 PAC，请人工确认是否保留"],
    },
    warnings: ["保存成功不代表代理可连接"],
  };
}

function section(status: SectionResult["status"], message: string): SectionResult {
  return {
    status,
    code: status === "failed" ? "PERMISSION_DENIED" : null,
    message,
  };
}

function makeReport(
  snapshot: ProxySnapshot,
  status: ApplyReport["status"] = "applied",
  terminal = section("applied", "终端代理已保存"),
  system = section("applied", "系统代理已保存"),
): ApplyReport {
  return { status, terminal, system, snapshot };
}

describe("代理设置首屏", () => {
  beforeEach(() => {
    getProxySnapshotMock.mockReset();
    applyProxySettingsMock.mockReset();
  });

  it("App_should区分持久进程与launchd值并脱敏_when读取成功", async () => {
    getProxySnapshotMock.mockResolvedValue(makeSnapshot());

    const wrapper = mount(App);
    await flushPromises();

    expect(getProxySnapshotMock).toHaveBeenCalledTimes(1);
    expect(wrapper.text()).toContain("应用进程值");
    expect(wrapper.text()).toContain("macOS launchd 用户会话值");
    expect(wrapper.text()).toContain("测试 Shell 配置区块");
    expect(wrapper.text()).toContain("检测到自动代理配置");
    expect(wrapper.text()).not.toContain("alice:secret");
    expect(wrapper.text()).not.toContain("process:secret");
    expect(wrapper.text()).not.toContain("launchd:secret");
    expect(wrapper.text()).not.toContain("pac:secret");
    expect((wrapper.find("#terminal-httpProxy").element as HTMLInputElement).value).toBe(
      "http://***:***@127.0.0.1:8080",
    );
  });

  it("App_should展示一次可行动错误并允许重试_when首屏读取失败", async () => {
    getProxySnapshotMock
      .mockRejectedValueOnce({
        message: "无法读取 http://alice:secret@127.0.0.1:8080",
      })
      .mockResolvedValueOnce(makeSnapshot());

    const wrapper = mount(App);
    await flushPromises();

    const alerts = wrapper.findAll('[role="alert"]');
    expect(alerts).toHaveLength(1);
    expect(alerts[0].text()).toContain("重新读取");
    expect(alerts[0].text()).toContain("http://***:***@127.0.0.1:8080");
    expect(alerts[0].text()).not.toContain("alice:secret");
    expect(wrapper.find("form").exists()).toBe(false);

    await wrapper.find("button").trigger("click");
    await flushPromises();
    expect(wrapper.find("form").exists()).toBe(true);
  });

  it("App_should在交互后同步视觉与aria验证_when端口非法", async () => {
    getProxySnapshotMock.mockResolvedValue(makeSnapshot());
    const wrapper = mount(App);
    await flushPromises();
    const port = wrapper.find("#system-http-port");

    expect(port.attributes("aria-invalid")).toBe("false");
    expect(wrapper.find("#system-http-port-error").exists()).toBe(false);

    await port.setValue("70000");
    expect(wrapper.find("#system-http-port-error").exists()).toBe(false);
    await port.trigger("blur");

    expect(port.attributes("aria-invalid")).toBe("true");
    expect(wrapper.find("#system-http-port-error").text()).toContain("1 到 65535");
  });
});

describe("useProxySettings", () => {
  beforeEach(() => {
    getProxySnapshotMock.mockReset();
    applyProxySettingsMock.mockReset();
  });

  it("草稿修改_should不反写权威快照", async () => {
    const initial = makeSnapshot();
    getProxySnapshotMock.mockResolvedValue(initial);
    const state = useProxySettings();
    await state.reload();

    state.terminalDraft.httpProxy = "http://draft.example:9000";
    state.markTerminalDirty("httpProxy");

    expect(state.snapshot.value?.terminal.persisted.httpProxy).toBe(
      "http://alice:secret@127.0.0.1:8080",
    );
    expect(state.terminalDraft.httpProxy).toBe("http://draft.example:9000");
  });

  it("saveAll_should拒绝重复提交_when上一次保存尚未完成", async () => {
    const initial = makeSnapshot();
    getProxySnapshotMock.mockResolvedValue(initial);
    let resolveApply: (report: ApplyReport) => void = () => undefined;
    applyProxySettingsMock.mockImplementation(
      () =>
        new Promise<ApplyReport>((resolve) => {
          resolveApply = resolve;
        }),
    );
    const state = useProxySettings();
    await state.reload();

    const first = state.saveAll();
    const second = state.saveAll();
    expect(applyProxySettingsMock).toHaveBeenCalledTimes(1);
    await expect(second).resolves.toBeNull();

    resolveApply(makeReport(initial));
    await expect(first).resolves.toMatchObject({ status: "applied" });
    expect(state.saving.value).toBe(false);
  });

  it("selectTarget_should传递macOS服务并用于后续保存", async () => {
    const wifi = makeSnapshot();
    const ethernet = makeSnapshot();
    ethernet.system.selectedTarget = "Ethernet";
    ethernet.system.http = { host: "10.0.0.2", port: 8080 };
    getProxySnapshotMock.mockResolvedValueOnce(wifi).mockResolvedValueOnce(ethernet);
    applyProxySettingsMock.mockResolvedValue(makeReport(ethernet));
    const state = useProxySettings();
    await state.reload();

    await state.selectTarget("Ethernet");
    expect(getProxySnapshotMock).toHaveBeenNthCalledWith(2, "Ethernet");
    expect(state.systemDraft.http.host).toBe("10.0.0.2");

    await state.saveAll();
    expect(applyProxySettingsMock).toHaveBeenCalledWith(
      expect.objectContaining({
        system: expect.objectContaining({ target: "Ethernet" }),
      }),
    );
  });

  it("saveAll_should保留失败区块草稿并更新成功区块_when部分成功", async () => {
    const initial = makeSnapshot();
    const nextSnapshot = makeSnapshot();
    nextSnapshot.terminal.persisted.httpProxy = "http://saved:secret@proxy.example:8080";
    nextSnapshot.system.http = { host: "192.0.2.10", port: 8080 };
    getProxySnapshotMock.mockResolvedValue(initial);
    applyProxySettingsMock.mockResolvedValue(
      makeReport(
        nextSnapshot,
        "partial",
        section("applied", "终端代理已保存"),
        section("failed", "系统代理权限不足，请授权后重试"),
      ),
    );
    const state = useProxySettings();
    await state.reload();
    state.terminalDraft.httpProxy = "http://terminal-draft.example:8080";
    state.markTerminalDirty("httpProxy");
    state.systemDraft.http.host = "system-draft.example";
    state.markSystemDirty();

    await state.saveAll();

    expect(state.terminalDraft.httpProxy).toBe("http://***:***@proxy.example:8080");
    expect(state.systemDraft.http.host).toBe("system-draft.example");
    expect(state.systemDirty.value).toBe(true);
    expect(state.snapshot.value?.system.http?.host).toBe("192.0.2.10");
  });

  it("saveAll_should脱敏全局错误并保留草稿_when命令失败", async () => {
    const initial = makeSnapshot();
    getProxySnapshotMock.mockResolvedValue(initial);
    applyProxySettingsMock.mockRejectedValue({
      message: "写入 http://alice:secret@127.0.0.1:8080 失败",
    });
    const state = useProxySettings();
    await state.reload();
    state.systemDraft.http.host = "draft.example";
    state.markSystemDirty();

    await state.saveAll();

    expect(state.saveError.value).toContain("http://***:***@127.0.0.1:8080");
    expect(state.saveError.value).not.toContain("alice:secret");
    expect(state.saveError.value).toContain("重新读取核对");
    expect(state.systemDraft.http.host).toBe("draft.example");
  });
});
