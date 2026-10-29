import { describe, expect, it } from 'vitest'
import { defaultCurrency } from './currency'

const CURRENCIES = ['CAD', 'EUR', 'GBP', 'INR', 'JPY', 'USD']

describe('defaultCurrency', () => {
  it("picks the currency of the locale's region", () => {
    expect(defaultCurrency('en-US', CURRENCIES)).toBe('USD')
    expect(defaultCurrency('en-GB', CURRENCIES)).toBe('GBP')
    expect(defaultCurrency('fr-CA', CURRENCIES)).toBe('CAD')
    expect(defaultCurrency('hi-IN', CURRENCIES)).toBe('INR')
  })

  it('knows the euro area', () => {
    for (const locale of ['de-DE', 'fr-FR', 'nl-NL', 'es-ES', 'it-IT', 'fi-FI', 'hr-HR', 'bg-BG']) {
      expect(defaultCurrency(locale, CURRENCIES), locale).toBe('EUR')
    }
  })

  it('guesses the region of a locale that names only a language', () => {
    expect(defaultCurrency('ja', CURRENCIES)).toBe('JPY')
    expect(defaultCurrency('de', CURRENCIES)).toBe('EUR')
  })

  it('falls back to USD for a region it has no currency for, or one not on the list', () => {
    expect(defaultCurrency('en-AQ', CURRENCIES)).toBe('USD')
    expect(defaultCurrency('pt-BR', CURRENCIES)).toBe('USD')
  })

  it('falls back to USD for a locale that is no locale', () => {
    expect(defaultCurrency('not a locale!', CURRENCIES)).toBe('USD')
  })
})
