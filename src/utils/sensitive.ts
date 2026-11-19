const URL_CREDENTIALS = /([a-z][a-z\d+.-]*:\/\/)([^\s/@]+)@/gi;

/** 只隐藏 URL 用户信息，保留协议、主机和端口以便核对代理目标。 */
export function maskSensitiveText(value: string): string {
  return value.replace(URL_CREDENTIALS, (_match, protocol: string, credentials: string) => {
    const maskedCredentials = credentials.includes(":") ? "***:***" : "***";
    return `${protocol}${maskedCredentials}@`;
  });
}

export function readableError(error: unknown): string {
  if (typeof error === "string") {
    return maskSensitiveText(error);
  }
  if (
    typeof error === "object" &&
    error !== null &&
    "message" in error &&
    typeof error.message === "string"
  ) {
    return maskSensitiveText(error.message);
  }
  return "未提供具体原因";
}
