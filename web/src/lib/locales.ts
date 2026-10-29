// The locales Settings › Profile offers for dates and numbers *(agent)*: common ones, and the
// browser's own. The app stays in English; only formats change.

const COMMON = `en-US en-GB en-CA en-AU en-NZ en-IE en-IN en-ZA de-DE de-AT de-CH fr-FR fr-CA fr-BE
fr-CH es-ES es-MX es-AR es-US it-IT nl-NL nl-BE pt-BR pt-PT sv-SE da-DK nb-NO fi-FI is-IS pl-PL
cs-CZ sk-SK hu-HU ro-RO el-GR tr-TR ru-RU uk-UA he-IL ar-SA hi-IN ja-JP ko-KR zh-CN zh-TW zh-HK
th-TH vi-VN id-ID ms-MY fil-PH`.split(/\s+/)

const names = new Intl.DisplayNames(['en'], { type: 'language', languageDisplay: 'standard' })

/** A locale as the picker shows it, like `German (Germany) · de-DE`. */
export function localeLabel(locale: string): string {
  let name: string | undefined
  try {
    name = names.of(locale)
  } catch {
    return locale
  }
  return name === undefined || name === locale ? locale : `${name} · ${locale}`
}

export interface LocaleOption {
  value: string
  label: string
}

/** The locales to choose from, `browser`'s among them, sorted by label. */
export function localeOptions(browser: string): LocaleOption[] {
  return [...new Set([browser, ...COMMON])]
    .map((locale) => ({ value: locale, label: localeLabel(locale) }))
    .sort((a, b) => a.label.localeCompare(b.label, 'en'))
}
