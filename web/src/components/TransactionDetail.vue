<script setup lang="ts">
// A transaction in full (docs/spec/m1-ledger-import.md#transactions): what the list shows, plus
// what the bank sent, the split lines, memo, status, source, how it got its category and the
// import it came in with. A From Actual, undecided row gets a category picker
// (docs/spec/m1-ledger-import.md#from-actual-undecided). A manual row no bank row has adopted
// carries the manual badge, and Edit (the entry form, in place of the details) and Delete, which
// is immediate: the screen offers Undo (docs/spec/m2-bank-data.md#manual-entry). A row from the
// bank can be deleted too, after a warning that suggests excluding it from budgets instead; its
// id is kept so sync never brings it back (docs/spec/m2-bank-data.md#deleting-a-synced-row).
// A row with a sync flag shows it first, with its action (docs/spec/m2-bank-data.md#sync-health-in-the-ui).
import { computed, ref, watch } from 'vue'
import { ApiError } from '@/api/client'
import { type ManualEntry, useManualEdits, useTransaction } from '@/api/transactions'
import { useDecide } from '@/api/undecided'
import CategoryPicker from '@/components/CategoryPicker.vue'
import GroupMark from '@/components/GroupMark.vue'
import ManualEntryForm from '@/components/ManualEntryForm.vue'
import Money from '@/components/Money.vue'
import SyncFlagPanel from '@/components/SyncFlagPanel.vue'
import { formatDate, useFormatting } from '@/lib/format'
import { type EntryErrors, entryOf, fieldsOf } from '@/lib/manual'
import { flagBadge } from '@/lib/syncFlags'
import { categoryLabel, payeeLabel, sourceLabel, statusLabel } from '@/lib/transactions'

const props = defineProps<{ id: number }>()
const emit = defineEmits<{
  close: []
  /** It was deleted; `entry` enters it again. */
  deleted: [entry: ManualEntry]
}>()

const query = useTransaction(() => props.id)
const formatting = useFormatting()
const transaction = computed(() => query.data.value)
// Not seen at the bank: its flag's panel has the Delete.
const notSeen = computed(() => transaction.value?.sync_flag === 'not_seen_at_bank')

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

const manual = useManualEdits()
const editing = ref(false)
const saving = ref(false)
const refused = ref<EntryErrors>()
// Deleting a bank row asks first.
const confirming = ref(false)
watch(
  () => props.id,
  () => {
    editing.value = false
    refused.value = undefined
    confirming.value = false
  },
)

async function save(entry: ManualEntry) {
  failure.value = ''
  saving.value = true
  try {
    await manual.change(props.id, entry)
    refused.value = undefined
    editing.value = false
  } catch (error) {
    if (error instanceof ApiError && error.code === 'validation_failed') {
      refused.value = error.fields
    } else {
      failure.value = error instanceof ApiError ? error.message : 'Something went wrong.'
    }
  } finally {
    saving.value = false
  }
}

async function removeBankRow() {
  failure.value = ''
  try {
    await manual.remove(props.id)
    confirming.value = false
    emit('close')
  } catch (error) {
    failure.value = error instanceof ApiError ? error.message : 'Something went wrong.'
  }
}

async function exclude(excluded: boolean) {
  failure.value = ''
  try {
    await manual.exclude(props.id, excluded)
    confirming.value = false
  } catch (error) {
    failure.value = error instanceof ApiError ? error.message : 'Something went wrong.'
  }
}

async function remove() {
  const shown = transaction.value
  if (!shown) return
  failure.value = ''
  try {
    await manual.remove(props.id)
    emit('deleted', entryOf(shown))
  } catch (error) {
    failure.value = error instanceof ApiError ? error.message : 'Something went wrong.'
  }
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
  <div class="flex flex-col gap-4">
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

    <ManualEntryForm
      v-if="transaction && editing"
      title="Edit transaction"
      action="Save changes"
      :start="fieldsOf(transaction, formatting)"
      :formatting="formatting"
      :refused="refused"
      :saving="saving"
      @save="save"
      @cancel="editing = false"
    />
    <template v-else-if="transaction">
      <p class="flex items-center gap-2 text-2xl">
        <Money :minor="transaction.amount_minor" signed />
        <span v-if="transaction.manual" class="rounded border px-1 text-xs text-muted-foreground"
          >Manual</span
        >
        <span
          v-if="transaction.sync_flag"
          class="rounded border border-primary px-1 text-xs text-primary"
          >{{ flagBadge(transaction.sync_flag) }}</span
        >
      </p>
      <SyncFlagPanel :transaction="transaction" @deleted="$emit('close')" />
      <div v-if="transaction.manual" class="flex gap-2">
        <button type="button" class="rounded-md border px-3 py-1" @click="editing = true">
          Edit
        </button>
        <button
          v-if="!notSeen"
          type="button"
          class="rounded-md border px-3 py-1 text-destructive"
          @click="remove"
        >
          Delete
        </button>
      </div>
      <div
        v-else-if="transaction.source === 'provider' && !confirming && !notSeen"
        class="flex gap-2"
      >
        <button
          type="button"
          class="rounded-md border px-3 py-1 text-destructive"
          @click="confirming = true"
        >
          Delete
        </button>
      </div>
      <section
        v-if="confirming"
        role="alertdialog"
        aria-labelledby="delete-bank-row"
        aria-describedby="delete-bank-row-why"
        class="flex flex-col gap-2 rounded-md border border-destructive p-3"
      >
        <h3 id="delete-bank-row" class="font-medium">Delete this bank transaction?</h3>
        <p id="delete-bank-row-why" class="text-muted-foreground">
          The bank still has it. Once deleted, syncing won't bring it back. To keep it in the list
          but out of your budgets, exclude it instead.
        </p>
        <div class="flex flex-wrap gap-2">
          <button
            v-if="!transaction.excluded"
            type="button"
            class="rounded-md bg-primary px-3 py-1 text-primary-foreground"
            @click="exclude(true)"
          >
            Exclude from budgets instead
          </button>
          <button
            type="button"
            class="rounded-md border px-3 py-1 text-destructive"
            @click="removeBankRow"
          >
            Delete
          </button>
          <button type="button" class="rounded-md border px-3 py-1" @click="confirming = false">
            Cancel
          </button>
        </div>
      </section>
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
            <li
              v-for="(line, index) in transaction.splits"
              :key="index"
              class="flex items-center gap-2"
            >
              <GroupMark :category="line.category_id" />
              <span class="flex-1">
                {{ line.category }}
                <span v-if="line.memo" class="text-muted-foreground">· {{ line.memo }}</span>
              </span>
              <Money :minor="line.amount_minor" signed />
            </li>
          </ul>
          <span v-else class="inline-flex items-center gap-1.5">
            <GroupMark :category="transaction.category_id" />
            {{ categoryLabel(transaction) }}
          </span>
        </dd>
        <dt class="text-muted-foreground">Memo</dt>
        <dd class="whitespace-pre-line">{{ transaction.memo ?? '—' }}</dd>
        <dt class="text-muted-foreground">Tags</dt>
        <dd>{{ transaction.tags.length > 0 ? transaction.tags.join(', ') : '—' }}</dd>
        <dt class="text-muted-foreground">Status</dt>
        <dd>{{ statusLabel(transaction.status) }}</dd>
        <dt class="text-muted-foreground">Source</dt>
        <dd>{{ sourceLabel(transaction.source) }}</dd>
        <dt class="text-muted-foreground">In budgets</dt>
        <dd class="flex flex-wrap items-center gap-2">
          {{ transaction.excluded ? 'Excluded' : 'Counted' }}
          <button
            v-if="transaction.excluded"
            type="button"
            class="rounded-md border px-3 py-1"
            @click="exclude(false)"
          >
            Count it again
          </button>
        </dd>
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
      </form>
    </template>
    <p v-if="failure" role="alert" class="w-full text-destructive">{{ failure }}</p>
  </div>
</template>
