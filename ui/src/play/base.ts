// Where the play page reads the world from. Empty: the server that served the page. With zone lines a player who
// walks through an exit lands in another zone, a map of its own on another server, and everything (info, walls,
// blocks, names) comes from that zone's web address (/play/info "zones") from then on.

let base = ''

/** The URL of a /play path on the zone we are in. */
export function playUrl(path: string): string {
  return base + path
}

/** Read from this zone's web address from now on ('' for the page's own server). */
export function setPlayBase(url: string) {
  base = url.replace(/\/+$/, '')
}
