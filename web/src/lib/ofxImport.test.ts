import { describe, expect, it } from 'vitest'
import { decodeFile, describeOfxRead, isOfx } from './ofxImport'

describe('isOfx', () => {
  it('knows OFX by its extension or its first bytes', () => {
    expect(isOfx('statement.QFX', '')).toBe(true)
    expect(isOfx('statement.qbo', '')).toBe(true)
    expect(isOfx('statement.txt', 'OFXHEADER:100\nDATA:OFXSGML')).toBe(true)
    expect(isOfx('statement.xml', '<?xml?>\n<OFX>\n')).toBe(true)
    expect(isOfx('statement.csv', 'Date,Description,Amount')).toBe(false)
  })
})

describe('decodeFile', () => {
  it('reads Windows-1252 when the header says so, and UTF-8 otherwise', () => {
    const western = new Uint8Array([
      ...new TextEncoder().encode('CHARSET:1252\n<OFX><NAME>CAF'),
      0xc9,
    ])
    expect(decodeFile(western)).toBe('CHARSET:1252\n<OFX><NAME>CAFÉ')
    expect(decodeFile(new TextEncoder().encode('<OFX><NAME>CAFÉ'))).toBe('<OFX><NAME>CAFÉ')
  })
})

describe('describeOfxRead', () => {
  it('says how many transactions, their dates and the account', () => {
    const read = {
      last_four: '0042',
      account_id: null,
      transactions: 3,
      first_date: '2026-09-03',
      last_date: '2026-09-20',
    }
    expect(describeOfxRead(read, { locale: 'en-US' })).toBe(
      '3 transactions, Sep 3, 2026 – Sep 20, 2026, for the account ending 0042',
    )
    expect(
      describeOfxRead(
        { ...read, transactions: 1, last_four: null, first_date: null, last_date: null },
        { locale: 'en-US' },
      ),
    ).toBe('1 transaction')
  })
})
