// @vitest-environment jsdom
import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ApplyReport, ProxySnapshot, SectionResult } from "../generated/bindings";

const {
  getProxySnapshotMock,
  applyProxySettingsMock,
  getGitProxySnapshotMock,
  applyGitProxySettingsMock,
} = vi.hoisted(() => ({
  getProxySnapshotMock: vi.fn(),
  applyProxySettingsMock: vi.fn(),
  getGitProxySnapshotMock: vi.fn(),
  applyGitProxySettingsMock: vi.fn(),
}));

vi.mock("../api/proxy", () => ({
  getProxySnapshot: getProxySnapshotMock,
  applyProxySettings: applyProxySettingsMock,
  getGitProxySnapshot: getGitProxySnapshotMock,
  applyGitProxySettings: applyGitProxySettingsMock,
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
  terminal: SectionResult | null = section("applied", "终端代理已保存"),
  system: SectionResult | null = section("applied", "系统代理已保存"),
): ApplyReport {
  return { status, terminal, system, snapshot };
}

const gitSnapshot = {
  available: true,
  writable: true,
  mode: "custom" as const,
  proxy: "http://***:***@proxy.example:8080",
  revision: "v1",
  warnings: [],
};

beforeEach(() => {
  vi.restoreAllMocks();
  getProxySnapshotMock.mockReset().mockResolvedValue(makeSnapshot());
  applyProxySettingsMock.mockReset().mockResolvedValue(makeReport(makeSnapshot()));
  getGitProxySnapshotMock.mockReset().mockResolvedValue(gitSnapshot);
  applyGitProxySettingsMock
    .mockReset()
    .mockResolvedValue({ status: "applied", message: "已保存并回读", snapshot: gitSnapshot });
  vi.spyOn(window, "confirm").mockReturnValue(true);
});

describe("三页签界面", () => {
  it("读取持久值并脱敏，默认折叠进程和来源", async () => {
    const wrapper = mount(App);
    await flushPromises();
    expect(getProxySnapshotMock).toHaveBeenCalledTimes(1);
    expect(wrapper.find("details").attributes("open")).toBeUndefined();
    expect((wrapper.find("#terminal-httpProxy").element as HTMLInputElement).value).toBe(
      "http://***:***@127.0.0.1:8080",
    );
    expect(wrapper.html()).not.toContain("alice:secret");
    expect(wrapper.html()).not.toContain("pac:secret");
  });

  it("切换不保存不丢草稿，箭头和Home/End更新选中及焦点", async () => {
    const wrapper = mount(App, { attachTo: document.body });
    await flushPromises();
    await wrapper.find("#terminal-httpProxy").setValue("http://draft.example:9000");
    await wrapper.find("#tab-terminal").trigger("keydown", { key: "End" });
    await flushPromises();
    expect(wrapper.find("#tab-git").attributes("aria-selected")).toBe("true");
    expect(document.activeElement?.id).toBe("tab-git");
    await wrapper.find("#tab-git").trigger("keydown", { key: "Home" });
    expect((wrapper.find("#terminal-httpProxy").element as HTMLInputElement).value).toBe(
      "http://draft.example:9000",
    );
    expect(applyProxySettingsMock).not.toHaveBeenCalled();
    wrapper.unmount();
  });

  it("系统端口失焦后展示验证，非法系统草稿不阻挡终端页保存", async () => {
    const wrapper = mount(App);
    await flushPromises();
    await wrapper.find("#tab-system").trigger("click");
    const port = wrapper.find("#system-http-port");
    await port.setValue("70000");
    expect(wrapper.find("#system-http-port-error").exists()).toBe(false);
    await port.trigger("blur");
    expect(port.attributes("aria-invalid")).toBe("true");
    await wrapper.find("#tab-terminal").trigger("click");
    await wrapper.find("form").trigger("submit");
    await flushPromises();
    expect(applyProxySettingsMock).toHaveBeenCalledWith({
      terminal: { httpProxy: null, httpsProxy: null, allProxy: null, noProxy: null },
      system: null,
    });
    expect((port.element as HTMLInputElement).value).toBe("70000");
  });

  it("首屏读取失败可重试且错误脱敏", async () => {
    getProxySnapshotMock.mockRejectedValueOnce("读取 http://alice:secret@proxy.example 失败");
    const wrapper = mount(App);
    await flushPromises();
    expect(wrapper.find('[role="alert"]').text()).not.toContain("alice:secret");
    await wrapper.find(".secondary-button").trigger("click");
    await flushPromises();
    expect(wrapper.find("#terminal-httpProxy").exists()).toBe(true);
  });

  it("Git页独立地址与保存边界", async () => {
    const wrapper = mount(App);
    await flushPromises();
    await wrapper.find("#terminal-noProxy").setValue("draft-only");
    await wrapper.find("#tab-git").trigger("click");
    await flushPromises();
    expect(wrapper.find("#git-proxy").attributes("type")).toBe("password");
    await wrapper.find("#git-proxy").setValue("http://new:credential@proxy.example:9000");
    await wrapper.find("form").trigger("submit");
    await flushPromises();
    expect(applyGitProxySettingsMock).toHaveBeenCalledWith({
      mode: "custom",
      proxy: "http://new:credential@proxy.example:9000",
      expectedRevision: "v1",
    });
    expect(applyProxySettingsMock).not.toHaveBeenCalled();
    expect((wrapper.find("#terminal-noProxy").element as HTMLInputElement).value).toBe(
      "draft-only",
    );
  });
});

it("保存中按钮禁用，切页后保留各页操作状态", async () => {
  let resolveSave!: (report: ApplyReport) => void;
  applyProxySettingsMock.mockImplementationOnce(
    () =>
      new Promise<ApplyReport>((resolve) => {
        resolveSave = resolve;
      }),
  );
  const wrapper = mount(App);
  await flushPromises();
  await wrapper.find("form").trigger("submit");
  expect(wrapper.find(".primary-button").attributes("disabled")).toBeDefined();
  expect(wrapper.find(".primary-button").text()).toContain("正在保存");
  await wrapper.find("#tab-system").trigger("click");
  expect(wrapper.find(".primary-button").attributes("disabled")).toBeUndefined();
  await wrapper.find("#tab-terminal").trigger("click");
  expect(wrapper.find(".primary-button").attributes("disabled")).toBeDefined();
  resolveSave(makeReport(makeSnapshot(), "applied", section("applied", "ok"), null));
  await flushPromises();
  expect(wrapper.find(".primary-button").attributes("disabled")).toBeUndefined();
});

it("macOS取消切换服务时，选择框和草稿均停留原服务", async () => {
  const wrapper = mount(App);
  await flushPromises();
  await wrapper.find("#tab-system").trigger("click");
  await wrapper.find("#system-http-host").setValue("draft.example");
  vi.mocked(window.confirm).mockReturnValue(false);
  await wrapper.find("#system-target").setValue("Ethernet");
  await flushPromises();
  expect((wrapper.find("#system-target").element as HTMLSelectElement).value).toBe("Wi-Fi");
  expect(getProxySnapshotMock).toHaveBeenCalledTimes(1);
});

describe("独立保存与回读", () => {
  it("终端保存只写终端，回读不覆盖系统权威快照和草稿", async () => {
    const state = useProxySettings();
    await state.reload();
    state.terminalDraft.httpProxy = "http://terminal.example:9000";
    state.markTerminalDirty("httpProxy");
    state.systemDraft.http.port = "70000";
    state.markSystemDirty();
    const next = makeSnapshot();
    next.system.http = { host: "unrelated.example", port: 1234 };
    applyProxySettingsMock.mockResolvedValue(
      makeReport(next, "applied", section("applied", "ok"), null),
    );
    await state.save("terminal");
    expect(applyProxySettingsMock.mock.calls[0][0].system).toBeNull();
    expect(state.systemDraft.http.port).toBe("70000");
    expect(state.systemDirty.value).toBe(true);
    expect(state.snapshot.value?.system.http?.host).toBe("127.0.0.1");
  });

  it("系统保存只带系统且不清除缺席终端草稿", async () => {
    const state = useProxySettings();
    await state.reload();
    state.terminalDraft.httpProxy = "draft";
    state.markTerminalDirty("httpProxy");
    state.systemDraft.http.host = "new.example";
    state.markSystemDirty();
    applyProxySettingsMock.mockResolvedValue(
      makeReport(makeSnapshot(), "applied", null, section("applied", "ok")),
    );
    await state.save("system");
    expect(applyProxySettingsMock.mock.calls[0][0].terminal).toBeNull();
    expect(state.terminalDraft.httpProxy).toBe("draft");
    expect(state.terminalDirty.httpProxy).toBe(true);
    expect(state.systemDirty.value).toBe(false);
  });

  it.each(["failed", "missing"])("%s结果保留当前草稿", async (result) => {
    const state = useProxySettings();
    await state.reload();
    state.terminalDraft.httpProxy = "http://draft.example:9000";
    state.markTerminalDirty("httpProxy");
    applyProxySettingsMock.mockResolvedValue(
      makeReport(
        makeSnapshot(),
        "failed",
        result === "missing" ? null : section("failed", "锁占用"),
        null,
      ),
    );
    await state.save("terminal");
    expect(state.terminalDraft.httpProxy).toBe("http://draft.example:9000");
    expect(state.dirty.value.terminal).toBe(true);
  });

  it("重新读取取消不调用命令、不丢草稿", async () => {
    const state = useProxySettings();
    await state.reload();
    state.terminalDraft.httpProxy = "draft";
    state.markTerminalDirty("httpProxy");
    vi.mocked(window.confirm).mockReturnValue(false);
    await state.reload("terminal");
    expect(getProxySnapshotMock).toHaveBeenCalledTimes(1);
    expect(applyProxySettingsMock).not.toHaveBeenCalled();
    expect(state.terminalDraft.httpProxy).toBe("draft");
  });

  it("重读当前页不覆盖其他页，重读失败保留当前草稿", async () => {
    const state = useProxySettings();
    await state.reload();
    state.systemDraft.http.host = "system-draft";
    state.markSystemDirty();
    await state.reload("terminal");
    expect(state.systemDraft.http.host).toBe("system-draft");
    state.terminalDraft.httpProxy = "terminal-draft";
    state.markTerminalDirty("httpProxy");
    getProxySnapshotMock.mockRejectedValueOnce("权限不足");
    await state.reload("terminal");
    expect(state.terminalDraft.httpProxy).toBe("terminal-draft");
    expect(state.dirty.value.terminal).toBe(true);
  });

  it("保存中拒绝重复提交且失败可以恢复", async () => {
    const state = useProxySettings();
    await state.reload();
    let rejectSave!: (error: unknown) => void;
    applyProxySettingsMock.mockImplementationOnce(
      () =>
        new Promise((_resolve, reject) => {
          rejectSave = reject;
        }),
    );
    const first = state.save("terminal");
    await state.save("terminal");
    expect(applyProxySettingsMock).toHaveBeenCalledTimes(1);
    expect(state.pages.terminal.saving).toBe(true);
    rejectSave("http://user:secret@proxy.example 失败");
    await first;
    expect(state.pages.terminal.saveError).not.toContain("user:secret");
    expect(state.pages.terminal.saving).toBe(false);
    await state.save("terminal");
    expect(applyProxySettingsMock).toHaveBeenCalledTimes(2);
  });

  it("macOS服务切换取消保留草稿，确认后只更新系统", async () => {
    const state = useProxySettings();
    await state.reload();
    state.systemDraft.http.host = "draft";
    state.markSystemDirty();
    vi.mocked(window.confirm).mockReturnValue(false);
    await state.selectTarget("Ethernet");
    expect(getProxySnapshotMock).toHaveBeenCalledTimes(1);
    expect(state.systemDraft.target).toBe("Wi-Fi");
    vi.mocked(window.confirm).mockReturnValue(true);
    const next = makeSnapshot();
    next.system.selectedTarget = "Ethernet";
    getProxySnapshotMock.mockResolvedValueOnce(next);
    await state.selectTarget("Ethernet");
    expect(state.systemDraft.target).toBe("Ethernet");
  });
});
