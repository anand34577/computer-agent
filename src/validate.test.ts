import { describe, expect, it } from "vitest";
import { hostScope, isLocalUrl, url } from "./validate";

describe("address checks", () => {
  it("classifies hosts", () => {
    expect(hostScope("127.0.0.1")).toBe("loopback");
    expect(hostScope("192.168.1.20")).toBe("private");
    expect(hostScope("api.openai.com")).toBe("public");
  });

  it("treats home-network addresses as local", () => {
    expect(isLocalUrl("http://192.168.1.20:1234/v1")).toBe(true);
    expect(isLocalUrl("https://api.anthropic.com")).toBe(false);
  });

  it("rejects non-web addresses", () => {
    expect(url("ftp://example.com")).toBeTruthy();
    expect(url("http://localhost:11434")).toBeUndefined();
  });
});
