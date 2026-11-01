import { describe, expect, it } from "vitest";
import { splitBlocks } from "./markdown";

describe("splitBlocks", () => {
  it("splits paragraphs at blank lines", () => {
    expect(splitBlocks("one\n\ntwo\nmore")).toEqual(["one", "two\nmore"]);
  });

  it("keeps blank lines inside code fences", () => {
    const md = "intro\n\n```py\na = 1\n\nb = 2\n```\n\nend";
    expect(splitBlocks(md)).toEqual(["intro", "```py\na = 1\n\nb = 2\n```", "end"]);
  });

  it("returns the unfinished block last, even inside an open fence", () => {
    expect(splitBlocks("done\n\n```js\nlet x")).toEqual(["done", "```js\nlet x"]);
  });
});
