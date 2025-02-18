type Pagination = {
  page: number
  perPage: number
}
export function withPaginator(input: Pagination) {
  return <T>(data: T) => ({
    data,
    meta: {
      page: input.page,
      perPage: input.perPage
    }
  })
}
export function withCursor(cursor: string) {
  return <T>(data: T) => ({
    data,
    meta: {
      cursor
    }
  })
}
