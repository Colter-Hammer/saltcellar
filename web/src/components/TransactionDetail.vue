<script setup lang="ts">
// A transaction in full (docs/spec/m1-ledger-import.md#transactions): what the list shows, plus
// what the bank sent, the split lines, memo, status, source, how it got its category and the
// import it came in with. Read-only in milestone 1, except that a From Actual, undecided row gets
// a category picker (docs/spec/m1-ledger-import.md#from-actual-undecided).
import { computed, ref } from 'vue'
import { ApiError } from '@/api/client'
import { useTransaction } from '@/api/transactions'
import { useDecide } from '@/api/undecided'
import CategoryPicker from '@/components/CategoryPicker.vue'
import { formatDate, formatMoney, useFormatting } from '@/lib/format'
import { categoryLabel, payeeLabel, sourceLabel, statusLabel } from '@/lib/transactions'

const props = defineProps<{ id: number }>()
defineEmits<{ close: [] }>()

const query = useTransaction(() => props.id)
const formatting = useFormatting()
const transaction = computed(() => query.data.value)

const decide = useDecide()
const chosen = ref('')
const failure = ref('')

async function setCategory() {
  failure.value = ''
  try {
    await decide.one.mutateAsync({ id: props.id, categoryId: Number(chosen.value) })
    chosen.value = ''
  } catch (error) {
    failure.value = error instanceof ApiError ? error.message : 'Something went wrong.'
  }
}

function money(minor: number, signed = false): string {
  return formatMoney(minor, formatting.value, { signed })
}

/** The import it came in with, like "export.zip, Feb 1, 2026". */
const batch = computed(() => {
  const found = transaction.value?.import_batch
  if (!found) return '—'
  const on = new Intl.DateTimeFormat(formatting.value.locale, { dateStyle: 'medium' }).format(
    new Date(found.imported_at),
  )
  return `${found.file_name}, ${on}`
})
</script>

<template>
  <div class="flex flex-col gap-4 text-sm">
    <div class="flex items-start gap-2">
      <h2 id="transaction-payee" class="min-w-0 flex-1 text-lg font-medium break-words">
        {{ transaction ? payeeLabel(transaction) : 'Transaction' }}
      </h2>
      <button
        type="button"
        class="rounded-md border px-2 py-1 text-muted-foreground"
        @click="$emit('close')"
      >
        Close
      </button>
    </div>

    <p v-if="query.isError.value" role="alert" class="text-destructive">
      {{ query.error.value?.message }}
    </p>

    <template v-if="transaction">
      <p class="text-2xl tabular-nums">{{ money(transaction.amount_minor, true) }}</p>
      <dl aria-label="Details" class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2">
        <dt class="text-muted-foreground">Date</dt>
        <dd>{{ formatDate(transaction.date, formatting) }}</dd>
        <dt class="text-muted-foreground">Account</dt>
        <dd>{{ transaction.account }}</dd>
        <dt class="text-muted-foreground">Payee</dt>
        <dd>{{ transaction.payee ?? '—' }}</dd>
        <dt class="text-muted-foreground">Bank description</dt>
        <dd class="font-mono text-xs break-all">{{ transaction.raw_description }}</dd>
        <dt class="text-muted-foreground">Category</dt>
        <dd>
          <ul v-if="transaction.splits.length > 0" aria-label="Split lines" class="flex flex-col">
            <li v-for="(line, index) in transaction.splits" :key="index" class="flex gap-2">
              <span class="flex-1">
                {{ line.category }}
                <span v-if="line.memo" class="text-muted-foreground">· {{ line.memo }}</span>
              </span>
              <span class="tabular-nums">{{ money(line.amount_minor) }}</span>
            </li>
          </ul>
          <template v-else>{{ categoryLabel(transaction) }}</template>
        </dd>
        <dt class="text-muted-foreground">Memo</dt>
        <dd class="whitespace-pre-line">{{ transaction.memo ?? '—' }}</dd>
        <dt class="text-muted-foreground">Tags</dt>
        <dd>{{ transaction.tags.length > 0 ? transaction.tags.join(', ') : '—' }}</dd>
        <dt class="text-muted-foreground">Status</dt>
        <dd>{{ statusLabel(transaction.status) }}</dd>
        <dt class="text-muted-foreground">Source</dt>
        <dd>{{ sourceLabel(transaction.source) }}</dd>
        <dt class="text-muted-foreground">Categorized</dt>
        <dd>{{ transaction.categorization_reason ?? '—' }}</dd>
        <dt class="text-muted-foreground">Import</dt>
        <dd>{{ batch }}</dd>
      </dl>
      <form
        v-if="transaction.undecided"
        aria-label="Set the category"
        class="flex flex-wrap items-center gap-2"
        @submit.prevent="setCategory"
      >
        <CategoryPicker v-model="chosen" label="Category" placeholder="Choose a category…" />
        <button
          type="submit"
          class="rounded-md bg-primary px-3 py-1 text-primary-foreground disabled:opacity-50"
          :disabled="chosen === '' || decide.one.isPending.value"
        >
          Set category
        </button>
        <p v-if="failure" role="alert" class="w-full text-destructive">{{ failure }}</p>
      </form>
    </template>
  </div>
</template>
