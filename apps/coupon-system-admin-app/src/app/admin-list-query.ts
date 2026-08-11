export interface AdminListQuery {
  filter: string;
  search: string;
  page: number;
}

export function normalizeAdminListQuery(
  input: Record<string, string | null | undefined>,
): AdminListQuery {
  const parsedPage = Number(input["page"] ?? 1);
  return {
    filter: input["filter"]?.trim() || "ALL",
    search: input["search"]?.trim() || "",
    page: Number.isInteger(parsedPage) && parsedPage > 0 ? parsedPage : 1,
  };
}

export function adminListQueryParams(
  query: AdminListQuery,
): Record<string, string> {
  return {
    filter: query.filter,
    search: query.search,
    page: String(query.page),
  };
}
