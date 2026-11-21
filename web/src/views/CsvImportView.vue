<script setup lang="ts">
// Import a file (docs/spec/m2-bank-data.md#csv-and-ofxqfx-import), for a CSV: file → mapping →
// preview → done. The file's text stays in the browser and goes along with each step, so the
// server keeps nothing between them *(agent)*. A file whose header the account has a saved mapping
// for skips the mapping step; nothing is written until Import on the preview step, which is one
// import batch that Settings › Imports can undo.
import { computed, ref } from 'vue'
import { useQueryClient } from '@tanstack/vue-query'
import { RouterLink, useRoute } from 'vue-router'
import { useAccounts } from '@/api/accounts'
import {
  type CsvImportDone,
  type CsvMapping,
  type CsvPreview,
  type CsvRead,
  commitCsvImport,
  previewCsvImport,
  readCsvFile,
} from '@/api/csvImport'
import Money from '@/components/Money.vue'
import {
  columnOptions,
  DATE_FORMATS,
  describeDone,
  describePreview,
  describeUnreadable,
  headerCells,
  keptRows,
  MAX_FILE_BYTES,
  startMapping,
  withAmountKind,
} from '@/lib/csvImport'

const steps = ['File', 'Mapping', 'Preview', 'Done']
const step = ref(1)
const route = useRoute()
const queryClient = useQueryClient()

const accounts = useAccounts()
// Closed accounts and balances-only ones (which keep no rows) can't take a file.
const choices = computed(() =>
  (accounts.data.value?.accounts ?? []).filter((a) => !a.closed && !a.balances_only),
)
const wanted = Number(route.query.account)
const accountId = ref<number | null>(Number.isInteger(wanted) && wanted > 0 ? wanted : null)
const accountName = computed(() => choices.value.find((a) => a.id === accountId.value)?.name ?? '')

const fileName = ref('')
const content = ref('')
const read = ref<CsvRead | null>(null)
const mapping = ref<CsvMapping | null>(null)
const usedSaved = ref(false)
const preview = ref<CsvPreview | null>(null)
const kept = ref<Set<number>>(new Set())
const done = ref<CsvImportDone | null>(null)
const busy = ref(false)
const error = ref('')

function message(failure: unknown): string {
  return failure instanceof Error ? failure.message : String(failure)
}

// ── Step 1: File ──

async function choose(event: Event) {
  const input = event.target
  if (!(input instanceof HTMLInputElement)) return
  const file = input.files?.[0]
  if (file === undefined) return
  // So choosing the same file again reads it again.
  input.value = ''
  error.value = ''
  if (accountId.value === null) {
    error.value = 'Choose the account first.'
    return
  }
  if (file.size > MAX_FILE_BYTES) {
    error.value = 'This file is over 10 MB, more than a bank statement should be.'
    return
  }
  busy.value = true
  try {
    const text = await file.text()
    const found = await readCsvFile(accountId.value, text)
    fileName.value = file.name
    content.value = text
    read.value = found
    mapping.value = startMapping(found)
    usedSaved.value = found.saved !== null
    kept.value = new Set()
    if (usedSaved.value) await showPreview()
    else step.value = 2
  } catch (failure) {
    error.value = message(failure)
  } finally {
    busy.value = false
  }
}

// ── Step 2: Mapping ──

const header = computed(() =>
  read.value && mapping.value ? headerCells(read.value, mapping.value) : [],
)
const options = computed(() => columnOptions(read.value?.columns ?? 0, header.value))

/** Edits the mapping through `change`, so each field's handler is one line. */
function edit(change: (current: CsvMapping) => CsvMapping) {
  if (mapping.value) mapping.value = change(mapping.value)
}

function picked(event: Event): number | null {
  const value = event.target instanceof HTMLSelectElement ? event.target.value : ''
  return value === '' ? null : Number(value)
}

function checked(event: Event): boolean {
  return event.target instanceof HTMLInputElement && event.target.checked
}

function skipRows(event: Event): number {
  const value = event.target instanceof HTMLInputElement ? Number(event.target.value) : 0
  return Number.isInteger(value) && value > 0 ? value : 0
}

function dateFormat(event: Event, current: CsvMapping['date_format']): CsvMapping['date_format'] {
  const value = event.target instanceof HTMLSelectElement ? event.target.value : ''
  return DATE_FORMATS.find((format) => format.value === value)?.value ?? current
}

function amountColumn(event: Event, which: 'column' | 'debit' | 'credit') {
  const value = picked(event) ?? 0
  edit((m) => {
    const { amount } = m
    if (amount.kind === 'signed') return { ...m, amount: { ...amount, column: value } }
    return {
      ...m,
      amount: which === 'credit' ? { ...amount, credit: value } : { ...amount, debit: value },
    }
  })
}

// ── Step 3: Preview ──

async function showPreview() {
  if (accountId.value === null || mapping.value === null) return
  error.value = ''
  busy.value = true
  try {
    preview.value = await previewCsvImport(accountId.value, content.value, mapping.value)
    kept.value = new Set()
    step.value = 3
  } catch (failure) {
    error.value = message(failure)
  } finally {
    busy.value = false
  }
}

function keep(row: number, on: boolean) {
  const next = new Set(kept.value)
  if (on) next.add(row)
  else next.delete(row)
  kept.value = next
}

function changeMapping() {
  usedSaved.value = false
  step.value = 2
}

function back() {
  error.value = ''
  step.value = usedSaved.value ? 1 : 2
}

async function commit() {
  if (accountId.value === null || mapping.value === null || preview.value === null) return
  error.value = ''
  busy.value = true
  try {
    done.value = await commitCsvImport({
      accountId: accountId.value,
      fileName: fileName.value,
      content: content.value,
      mapping: mapping.value,
      keep: keptRows(preview.value.check, kept.value),
    })
    // Rows, balances and the imports list all changed.
    await queryClient.invalidateQueries()
    step.value = 4
  } catch (failure) {
    error.value = message(failure)
  } finally {
    busy.value = false
  }
}

function another() {
  read.value = mapping.value = preview.value = done.value = null
  fileName.value = content.value = error.value = ''
  step.value = 1
}

const selectClass = 'rounded-md border bg-background px-3 py-2'
const buttonClass = 'inline-flex items-center rounded-md border px-3 py-2 disabled:opacity-50'
</script>

<template>
  <div class="flex max-w-3xl flex-col gap-5 p-4 lg:max-w-5xl lg:px-6">
    <ol aria-label="Steps" class="flex gap-1 text-xs">
      <li
        v-for="(name, i) in steps"
        :key="name"
        class="flex-1"
        :aria-current="i + 1 === step ? 'step' : undefined"
      >
        <div class="h-1 rounded-full" :class="i + 1 <= step ? 'bg-primary' : 'bg-muted'" />
        <div class="mt-1" :class="i + 1 === step ? 'font-medium' : 'text-muted-foreground'">
          {{ i + 1 }}. {{ name }}
        </div>
      </li>
    </ol>

    <p v-if="error" role="alert" class="text-sm text-destructive">{{ error }}</p>

    <section v-if="step === 1" aria-label="File" class="flex flex-col gap-4">
      <div class="flex flex-col gap-1.5">
        <label for="csv-account" class="text-sm font-medium">Account</label>
        <select id="csv-account" v-model="accountId" :class="selectClass" class="lg:max-w-sm">
          <option :value="null" disabled>Choose an account</option>
          <option v-for="account in choices" :key="account.id" :value="account.id">
            {{ account.name }}
          </option>
        </select>
      </div>
      <div class="flex flex-col gap-1.5">
        <label for="csv-file" class="text-sm font-medium">CSV file</label>
        <p id="csv-file-hint" class="text-sm text-muted-foreground">
          A file from your bank's site. Nothing is imported until you've seen the preview and chosen
          Import.
        </p>
        <input
          id="csv-file"
          type="file"
          accept=".csv,text/csv,text/plain"
          aria-describedby="csv-file-hint"
          class="rounded-md border p-2"
          :disabled="busy"
          @change="choose"
        />
      </div>
      <p v-if="busy" role="status" class="text-sm text-muted-foreground">Reading…</p>
    </section>

    <section
      v-else-if="step === 2 && read && mapping"
      aria-label="Mapping"
      class="flex flex-col gap-4"
    >
      <p class="text-sm text-muted-foreground">
        Say which column holds what in {{ fileName }}. The mapping is saved for {{ accountName }},
        so a file with the same header skips this step.
      </p>

      <div class="overflow-x-auto rounded-xl border">
        <table aria-label="The file's first rows" class="w-full text-left text-sm">
          <tbody>
            <tr
              v-for="(record, at) in read.records.slice(0, 8)"
              :key="at"
              :class="[
                at < mapping.skip_rows ? 'bg-muted text-muted-foreground' : '',
                at === mapping.skip_rows - 1 ? 'font-medium' : '',
              ]"
            >
              <td
                v-for="(cell, column) in record"
                :key="column"
                class="px-3 py-1.5 whitespace-nowrap"
              >
                {{ cell }}
              </td>
            </tr>
          </tbody>
        </table>
      </div>

      <div class="grid gap-4 sm:grid-cols-2">
        <div class="flex flex-col gap-1.5">
          <label for="map-skip" class="text-sm font-medium">Rows to skip, header included</label>
          <input
            id="map-skip"
            type="number"
            min="0"
            :max="read.record_count"
            :value="mapping.skip_rows"
            class="rounded-md border px-3 py-2"
            @input="(event) => edit((m) => ({ ...m, skip_rows: skipRows(event) }))"
          />
        </div>

        <div class="flex flex-col gap-1.5">
          <label for="map-format" class="text-sm font-medium">Date format</label>
          <select
            id="map-format"
            :value="mapping.date_format"
            :class="selectClass"
            @change="
              (event) => edit((m) => ({ ...m, date_format: dateFormat(event, m.date_format) }))
            "
          >
            <option v-for="f in DATE_FORMATS" :key="f.value" :value="f.value">{{ f.label }}</option>
          </select>
        </div>

        <div class="flex flex-col gap-1.5">
          <label for="map-date" class="text-sm font-medium">Date column</label>
          <select
            id="map-date"
            :value="mapping.date"
            :class="selectClass"
            @change="(event) => edit((m) => ({ ...m, date: picked(event) ?? m.date }))"
          >
            <option v-for="o in options" :key="o.value" :value="o.value">{{ o.label }}</option>
          </select>
        </div>

        <div class="flex flex-col gap-1.5">
          <label for="map-post" class="text-sm font-medium">Post date column (optional)</label>
          <select
            id="map-post"
            :value="mapping.post_date ?? ''"
            :class="selectClass"
            @change="(event) => edit((m) => ({ ...m, post_date: picked(event) }))"
          >
            <option value="">None</option>
            <option v-for="o in options" :key="o.value" :value="o.value">{{ o.label }}</option>
          </select>
        </div>

        <div class="flex flex-col gap-1.5">
          <label for="map-description" class="text-sm font-medium">Description column</label>
          <select
            id="map-description"
            :value="mapping.description"
            :class="selectClass"
            @change="
              (event) => edit((m) => ({ ...m, description: picked(event) ?? m.description }))
            "
          >
            <option v-for="o in options" :key="o.value" :value="o.value">{{ o.label }}</option>
          </select>
        </div>

        <fieldset class="flex flex-col gap-2">
          <legend class="text-sm font-medium">Amount</legend>
          <label class="flex items-center gap-2">
            <input
              type="radio"
              name="amount-kind"
              :checked="mapping.amount.kind === 'signed'"
              @change="edit((m) => withAmountKind(m, 'signed', read?.columns ?? 0))"
            />
            One column, with a sign
          </label>
          <label class="flex items-center gap-2">
            <input
              type="radio"
              name="amount-kind"
              :checked="mapping.amount.kind === 'debit_credit'"
              @change="edit((m) => withAmountKind(m, 'debit_credit', read?.columns ?? 0))"
            />
            Debit and credit columns
          </label>
        </fieldset>

        <div v-if="mapping.amount.kind === 'signed'" class="flex flex-col gap-1.5">
          <label for="map-amount" class="text-sm font-medium">Amount column</label>
          <select
            id="map-amount"
            :value="mapping.amount.column"
            :class="selectClass"
            @change="(event) => amountColumn(event, 'column')"
          >
            <option v-for="o in options" :key="o.value" :value="o.value">{{ o.label }}</option>
          </select>
        </div>
        <template v-else>
          <div class="flex flex-col gap-1.5">
            <label for="map-debit" class="text-sm font-medium">Debit column (money out)</label>
            <select
              id="map-debit"
              :value="mapping.amount.debit"
              :class="selectClass"
              @change="(event) => amountColumn(event, 'debit')"
            >
              <option v-for="o in options" :key="o.value" :value="o.value">{{ o.label }}</option>
            </select>
          </div>
          <div class="flex flex-col gap-1.5">
            <label for="map-credit" class="text-sm font-medium">Credit column (money in)</label>
            <select
              id="map-credit"
              :value="mapping.amount.credit"
              :class="selectClass"
              @change="(event) => amountColumn(event, 'credit')"
            >
              <option v-for="o in options" :key="o.value" :value="o.value">{{ o.label }}</option>
            </select>
          </div>
        </template>

        <label class="flex items-center gap-2 sm:col-span-2">
          <input
            type="checkbox"
            :checked="mapping.flip_sign"
            @change="(event) => edit((m) => ({ ...m, flip_sign: checked(event) }))"
          />
          Flip the sign (the file writes money out as positive)
        </label>

        <div class="flex flex-col gap-1.5">
          <label for="map-category" class="text-sm font-medium">Category column (optional)</label>
          <select
            id="map-category"
            :value="mapping.category ?? ''"
            :class="selectClass"
            @change="(event) => edit((m) => ({ ...m, category: picked(event) }))"
          >
            <option value="">None</option>
            <option v-for="o in options" :key="o.value" :value="o.value">{{ o.label }}</option>
          </select>
        </div>

        <div class="flex flex-col gap-1.5">
          <label for="map-memo" class="text-sm font-medium">Memo column (optional)</label>
          <select
            id="map-memo"
            :value="mapping.memo ?? ''"
            :class="selectClass"
            @change="(event) => edit((m) => ({ ...m, memo: picked(event) }))"
          >
            <option value="">None</option>
            <option v-for="o in options" :key="o.value" :value="o.value">{{ o.label }}</option>
          </select>
        </div>
      </div>

      <div class="flex justify-between gap-2">
        <button type="button" :class="buttonClass" :disabled="busy" @click="another">Back</button>
        <button type="button" :class="buttonClass" :disabled="busy" @click="showPreview">
          Preview
        </button>
      </div>
    </section>

    <section v-else-if="step === 3 && preview" aria-label="Preview" class="flex flex-col gap-4">
      <p v-if="usedSaved" class="text-sm text-muted-foreground">
        Using the mapping saved for {{ accountName }}.
        <button type="button" class="underline" @click="changeMapping">Change the mapping</button>
      </p>
      <h2 class="text-lg font-medium" data-testid="preview-summary">
        {{ describePreview(preview) }}
      </h2>
      <p v-if="preview.unreadable > 0" class="text-sm text-muted-foreground">
        {{ describeUnreadable(preview.unreadable) }}
        <template v-for="problem in preview.problems.slice(0, 3)" :key="problem.row">
          Row {{ problem.row }}: {{ problem.why }}.
        </template>
      </p>

      <div v-if="preview.sample.length > 0" class="flex flex-col gap-1">
        <h3 class="text-sm font-medium">New rows</h3>
        <div class="overflow-x-auto rounded-xl border">
          <table aria-label="New rows" class="w-full text-left text-sm">
            <tbody class="divide-y">
              <tr v-for="row in preview.sample" :key="row.row">
                <td class="px-3 py-2 whitespace-nowrap">{{ row.date }}</td>
                <td class="px-3 py-2">{{ row.description }}</td>
                <td class="px-3 py-2 text-right"><Money :minor="row.amount_minor" signed /></td>
              </tr>
            </tbody>
          </table>
        </div>
        <p v-if="preview.new > preview.sample.length" class="text-sm text-muted-foreground">
          …and {{ preview.new - preview.sample.length }} more.
        </p>
      </div>

      <div v-if="preview.check.length > 0" class="flex flex-col gap-2">
        <h3 class="text-sm font-medium">To check</h3>
        <p class="text-sm text-muted-foreground">
          These could be rows {{ accountName }} has already. Tick the ones that are really new.
        </p>
        <ul aria-label="To check" class="divide-y rounded-xl border">
          <li v-for="row in preview.check" :key="row.row" class="flex flex-col gap-1 p-3">
            <label class="flex items-start gap-3">
              <input
                type="checkbox"
                class="mt-1"
                :checked="kept.has(row.row)"
                @change="(event) => keep(row.row, checked(event))"
              />
              <span class="flex flex-col">
                <span>
                  {{ row.date }} · {{ row.description }} ·
                  <Money :minor="row.amount_minor" signed />
                </span>
                <span v-if="row.could_be" class="text-sm text-muted-foreground">
                  Could be {{ row.could_be.date }} · {{ row.could_be.description }}
                </span>
              </span>
              <span class="sr-only">Add as a new row</span>
            </label>
          </li>
        </ul>
      </div>

      <div class="flex justify-between gap-2">
        <button type="button" :class="buttonClass" :disabled="busy" @click="back">Back</button>
        <button
          type="button"
          class="inline-flex items-center rounded-md bg-primary px-3 py-2 text-primary-foreground disabled:opacity-50"
          :disabled="busy"
          @click="commit"
        >
          Import
        </button>
      </div>
    </section>

    <section v-else-if="step === 4 && done" aria-label="Imported" class="flex flex-col gap-4">
      <h2 class="text-lg font-medium">Imported {{ fileName }}</h2>
      <p data-testid="done-summary">{{ describeDone(done) }}</p>
      <div class="flex flex-wrap gap-2">
        <RouterLink
          :to="{ path: '/transactions', query: { account: String(accountId) } }"
          :class="buttonClass"
        >
          See transactions
        </RouterLink>
        <RouterLink to="/settings/imports" :class="buttonClass"
          >Undo in Settings › Imports</RouterLink
        >
        <button type="button" :class="buttonClass" @click="another">Import another file</button>
      </div>
    </section>
  </div>
</template>
