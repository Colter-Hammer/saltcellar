<script setup lang="ts">
// A row's sync flag and its action (docs/spec/m2-bank-data.md#sync-health-in-the-ui), at the top of
// its detail: Merge / Keep both a possible duplicate, Yes / No a possible manual match, Delete /
// Keep a row not seen at the bank, and Fix the split whose amount the bank changed (Review takes
// that over in milestone 3). Each action clears the flag *(agent: the words)*.
import { computed, ref, watch } from 'vue'
import { ApiError } from '@/api/client'
import { type FlagAction, useFlagActions } from '@/api/syncFlags'
import { type Transaction, useManualEdits } from '@/api/transactions'
import SplitFixForm from '@/components/SplitFixForm.vue'
import { formatDate, formatMoney, useFormatting } from '@/lib/format'
import { payeeLabel } from '@/lib/transactions'

const props = defineProps<{ transaction: Transaction }>()
const emit = defineEmits<{ deleted: [] }>()

const formatting = useFormatting()
const actions = useFlagActions()
const edits = useManualEdits()
const busy = ref(false)
const failure = ref('')
const fixing = ref(false)
watch(
  () => props.transaction.id,
  () => {
    fixing.value = false
    failure.value = ''
  },
)

const flag = computed(() => props.transaction.sync_flag)
const other = computed(() => props.transaction.flagged_against)
const money = (minor: number) => formatMoney(minor, formatting.value)
/** The other row, like "Corner Market, −€12.50 on Oct 27, 2026". */
const otherText = computed(() => {
  const row = other.value
  if (!row) return ''
  return `${payeeLabel(row)}, ${formatMoney(row.amount_minor, formatting.value, { signed: true })} on ${formatDate(row.date, formatting.value)}`
})
const splitTotal = computed(() =>
  props.transaction.splits.reduce((sum, line) => sum + line.amount_minor, 0),
)

async function run(work: () => Promise<unknown>) {
  failure.value = ''
  busy.value = true
  try {
    await work()
  } catch (error) {
    failure.value = error instanceof ApiError ? error.message : 'Something went wrong.'
  } finally {
    busy.value = false
  }
}

const resolve = (action: FlagAction) => run(() => actions.resolve(props.transaction.id, action))
const remove = () =>
  run(async () => {
    await edits.remove(props.transaction.id)
    emit('deleted')
  })
</script>

<template>
  <section
    v-if="flag"
    aria-labelledby="sync-flag"
    class="flex flex-col gap-2 rounded-md border border-primary p-3"
  >
    <template v-if="flag === 'possible_duplicate'">
      <h3 id="sync-flag" class="font-medium">Possible duplicate</h3>
      <p class="text-muted-foreground">
        It may be the same transaction as {{ otherText || 'another one' }}. Merge keeps this one,
        with your category, memo and tags.
      </p>
      <div class="flex flex-wrap gap-2">
        <button
          type="button"
          class="rounded-md bg-primary px-3 py-1 text-primary-foreground"
          :disabled="busy || !other"
          @click="resolve('merge')"
        >
          Merge
        </button>
        <button
          type="button"
          class="rounded-md border px-3 py-1"
          :disabled="busy"
          @click="resolve('keep_both')"
        >
          Keep both
        </button>
      </div>
    </template>

    <template v-else-if="flag === 'possible_manual_match'">
      <h3 id="sync-flag" class="font-medium">
        Is this your {{ other ? money(Math.abs(other.amount_minor)) : '' }} manual entry?
      </h3>
      <p class="text-muted-foreground">
        {{ otherText }}. Yes keeps the bank's amount with your category, memo and tags.
      </p>
      <div class="flex flex-wrap gap-2">
        <button
          type="button"
          class="rounded-md bg-primary px-3 py-1 text-primary-foreground"
          :disabled="busy || !other"
          @click="resolve('accept_match')"
        >
          Yes
        </button>
        <button
          type="button"
          class="rounded-md border px-3 py-1"
          :disabled="busy"
          @click="resolve('reject_match')"
        >
          No
        </button>
      </div>
    </template>

    <template v-else-if="flag === 'not_seen_at_bank'">
      <h3 id="sync-flag" class="font-medium">Not seen at the bank</h3>
      <p class="text-muted-foreground">
        {{
          transaction.manual
            ? 'No bank transaction has matched this entry in two weeks.'
            : "The bank hasn't sent this transaction for over a week."
        }}
        Delete it, or keep it as it is.
      </p>
      <div class="flex flex-wrap gap-2">
        <button
          type="button"
          class="rounded-md border px-3 py-1 text-destructive"
          :disabled="busy"
          @click="remove"
        >
          Delete
        </button>
        <button
          type="button"
          class="rounded-md border px-3 py-1"
          :disabled="busy"
          @click="resolve('keep')"
        >
          Keep
        </button>
      </div>
    </template>

    <template v-else>
      <h3 id="sync-flag" class="font-medium">
        Amount changed from {{ money(splitTotal) }} to {{ money(transaction.amount_minor) }}, fix
        the split
      </h3>
      <SplitFixForm
        v-if="fixing"
        :transaction="transaction"
        @done="fixing = false"
        @cancel="fixing = false"
      />
      <div v-else>
        <button
          type="button"
          class="rounded-md bg-primary px-3 py-1 text-primary-foreground"
          @click="fixing = true"
        >
          Fix the split
        </button>
      </div>
    </template>
    <p v-if="failure" role="alert" class="text-destructive">{{ failure }}</p>
  </section>
</template>
