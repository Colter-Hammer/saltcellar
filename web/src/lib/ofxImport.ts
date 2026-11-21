// The OFX side of Import a file (docs/spec/m2-bank-data.md#csv-and-ofxqfx-import): telling an
// OFX, QFX or QBO file from a CSV one, reading its bytes in the charset it says, and the line the
// File step shows about it. Nothing here talks to the server.
import type { OfxRead } from '@/api/ofxImport'
import { formatDate, type Formatting } from './format'

const OFX_NAME = /\.(ofx|qfx|qbo)$/i
const OFX_TEXT = /OFXHEADER|<OFX[\s>]/i

/** Whether a file is OFX: by its name's extension, or by its first bytes saying so. */
export function isOfx(name: string, start: string): boolean {
  return OFX_NAME.test(name) || OFX_TEXT.test(start)
}

/**
 * A file's text: Windows-1252 when its header says so (an OFX 1.x `CHARSET:1252`, or an XML
 * `encoding="windows-1252"` or ISO-8859-1), else UTF-8 *(agent)*.
 */
export function decodeFile(bytes: Uint8Array): string {
  const head = new TextDecoder('latin1').decode(bytes.subarray(0, 2048))
  const western =
    /CHARSET:\s*(1252|ISO-8859-1|8859-1)\b/i.test(head) ||
    /encoding=["']?(windows-1252|iso-8859-1)/i.test(head)
  return new TextDecoder(western ? 'windows-1252' : 'utf-8').decode(bytes)
}

/** "3 transactions, Sep 3, 2026 – Sep 20, 2026, for the account ending 0042". */
export function describeOfxRead(read: OfxRead, { locale }: Pick<Formatting, 'locale'>): string {
  const count = `${new Intl.NumberFormat(locale).format(read.transactions)} ${read.transactions === 1 ? 'transaction' : 'transactions'}`
  const dates =
    read.first_date === null || read.last_date === null
      ? ''
      : `, ${formatDate(read.first_date, { locale })} – ${formatDate(read.last_date, { locale })}`
  const account = read.last_four === null ? '' : `, for the account ending ${read.last_four}`
  return `${count}${dates}${account}`
}
