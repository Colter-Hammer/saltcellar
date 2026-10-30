// What the importer's File step says about an uploaded Actual export.
import type { ActualUpload } from '@/api/imports'
import { formatDate, type Formatting } from './format'

/** `count` and the noun, singular or plural: "1 account", "1,234 transactions". */
function counted(count: number, [one, many]: [string, string], locale: string): string {
  return `${new Intl.NumberFormat(locale).format(count)} ${count === 1 ? one : many}`
}

/** The upload's counts, and the dates its transactions span. */
export function describeUpload(
  upload: ActualUpload,
  { locale }: Pick<Formatting, 'locale'>,
): { counts: string; dates: string } {
  const counts = [
    counted(upload.transactions, ['transaction', 'transactions'], locale),
    counted(upload.accounts, ['account', 'accounts'], locale),
    counted(upload.categories, ['category', 'categories'], locale),
  ].join(' · ')
  const dates =
    upload.first_date === null || upload.last_date === null
      ? 'No transactions'
      : `${formatDate(upload.first_date, { locale })} – ${formatDate(upload.last_date, { locale })}`
  return { counts, dates }
}
