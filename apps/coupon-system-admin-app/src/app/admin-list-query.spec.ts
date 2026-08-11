import { describe, expect, it } from "vitest";
import {
  adminListQueryParams,
  normalizeAdminListQuery,
} from "./admin-list-query";

describe("admin list URL query", () => {
  it("preserves filter, search and page across a reload", () => {
    const first = normalizeAdminListQuery({
      filter: "FAILED_PERMANENT",
      search: "template-42",
      page: "3",
    });
    const url = adminListQueryParams(first);
    const restored = normalizeAdminListQuery(url);
    expect(restored).toEqual(first);
  });

  it("normalizes an invalid page without losing the active filter", () => {
    expect(
      normalizeAdminListQuery({ filter: "PENDING", search: "", page: "-2" }),
    ).toEqual({ filter: "PENDING", search: "", page: 1 });
  });
});
