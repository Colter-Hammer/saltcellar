<script setup lang="ts">
// An account's drift (docs/spec/m2-bank-data.md#balances): when the bank's balance and the app's
// stayed apart by the same amount for two snapshots, "Balance off by $X since <date>" with the
// bank's balance against the app's, **See suspect rows** (what the sync that stepped it added,
// changed or dropped) and **Accept $X as balance adjustment** with an optional note. Below, the
// adjustments accepted so far, each with **Undo** until it is undone; an undone one stays listed
// *(agent: the words and the layout)*.
import { computed, ref, watch } from 'vue'
import { type Account, useAccountEdits } from '@/api/accounts'
import { ApiError } from '@/api/client'
import Money from '@/components/Money.vue'
import { formatDate, formatMoney, useFormatting } from '@/lib/format'

const props = defineProps<{ account: Account }>()

const formatting = useFormatting()
const edits = useAccountEdits()
const flag = computed(() => props.account.drift.flag)
const comparison = computed(() => props.account.drift.bank_comparison)
const showRows = ref(false)
const accepting = ref(false)
const note = ref('')
const busy = ref(false)
const failure = ref('')
watch(
  () => flag.value?.drift_minor,
  () => {
    accepting.value = false
    note.value = ''
  },
)

const off = computed(() =>
  flag.value ? formatMoney(flag.value.drift_minor, formatting.value) : '',
)
const since = computed(() => (flag.value ? formatDate(flag.value.since, formatting.value) : ''))
const CHANGES = { added: 'Added', changed: 'Changed', dropped: 'Dropped' } as const
type Adjustment = Account['drift']['adjustments'][number]
/** Its note, who accepted it and whether it's undone, like "Monthly fee · Sam · Undone". */
const about = (adjustment: Adjustment) =>
  [adjustment.note ?? 'No note', adjustment.accepted_by, adjustment.undone_at ? 'Undone' : null]
    .filter((part) => part !== null)
    .join(' · ')

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

const accept = () =>
  run(async () => {
    const drift = flag.value
    if (!drift) return
    await edits.acceptAdjustment(props.account.id, drift.drift_minor, note.value)
    accepting.value = false
    note.value = ''
  })
const undo = (id: number) => run(() => edits.undoAdjustment(id))
</script>

<template>
  <section
    v-if="flag"
    data-testid="drift-flag"
    aria-labelledby="drift-heading"
    class="flex flex-col gap-3 rounded-xl border border-destructive p-4 text-sm lg:max-w-md"
  >
    <h3 id="drift-heading" class="font-medium text-destructive">
      Balance off by {{ off }} since {{ since }}
    </h3>
    <dl v-if="comparison" class="grid grid-cols-[auto_1fr] gap-x-6 gap-y-1">
      <dt class="text-muted-foreground">Bank says</dt>
      <dd class="text-right"><Money :minor="comparison.bank_minor" /></dd>
      <dt class="text-muted-foreground">App says</dt>
      <dd class="text-right"><Money :minor="comparison.app_minor" /></dd>
    </dl>
    <p class="text-muted-foreground">
      Fix the rows if one is wrong or missing. If nothing explains it, accept the difference.
    </p>
    <div class="flex flex-wrap gap-2">
      <button
        type="button"
        class="rounded-md border px-3 py-1"
        :aria-expanded="showRows"
        @click="showRows = !showRows"
      >
        {{ showRows ? 'Hide suspect rows' : 'See suspect rows' }}
      </button>
      <button
        v-if="!accepting"
        type="button"
        class="rounded-md bg-primary px-3 py-1 text-primary-foreground"
        :disabled="busy"
        @click="accepting = true"
      >
        Accept {{ off }} as balance adjustment
      </button>
    </div>
    <div v-if="showRows" data-testid="suspect-rows">
      <p v-if="flag.suspect_rows.length === 0" class="text-muted-foreground">
        That sync didn't add, change or drop any rows on this account.
      </p>
      <ul v-else class="flex flex-col divide-y rounded-md border">
        <li
          v-for="(row, index) in flag.suspect_rows"
          :key="index"
          class="flex items-baseline gap-2 px-3 py-2"
        >
          <span class="w-14 shrink-0 text-xs text-muted-foreground">{{ CHANGES[row.change] }}</span>
          <span class="min-w-0 flex-1">
            <RouterLink
              v-if="row.transaction_id !== null"
              :to="`/transactions/${row.transaction_id}`"
              class="hover:underline"
              >{{ row.description || 'No description' }}</RouterLink
            >
            <span v-else>{{ row.description || 'No description' }}</span>
            <span class="block text-xs text-muted-foreground">{{
              formatDate(row.date, formatting)
            }}</span>
          </span>
          <Money :minor="row.amount_minor" signed />
        </li>
      </ul>
    </div>
    <form v-if="accepting" class="flex flex-col gap-2" @submit.prevent="accept">
      <label class="flex flex-col gap-1">
        <span class="text-muted-foreground">Note (optional)</span>
        <input
          v-model="note"
          type="text"
          maxlength="500"
          class="rounded-md border bg-background px-3 py-2"
          placeholder="Why the balance is off"
        />
      </label>
      <div class="flex flex-wrap gap-2">
        <button
          type="submit"
          class="rounded-md bg-primary px-3 py-1 text-primary-foreground"
          :disabled="busy"
        >
          Accept {{ off }}
        </button>
        <button
          type="button"
          class="rounded-md border px-3 py-1"
          @click="((accepting = false), (note = ''))"
        >
          Cancel
        </button>
      </div>
    </form>
    <p v-if="failure" role="alert" class="text-destructive">{{ failure }}</p>
  </section>

  <section
    v-if="account.drift.adjustments.length > 0"
    aria-labelledby="adjustments-heading"
    class="flex flex-col gap-2 text-sm lg:max-w-md"
  >
    <h3 id="adjustments-heading" class="font-medium">Balance adjustments</h3>
    <ul class="flex flex-col divide-y rounded-xl border">
      <li
        v-for="adjustment in account.drift.adjustments"
        :key="adjustment.id"
        data-testid="adjustment"
        class="flex items-center gap-3 px-3 py-2"
        :class="adjustment.undone_at ? 'text-muted-foreground' : ''"
      >
        <span class="min-w-0 flex-1">
          <span :class="adjustment.undone_at ? 'line-through' : ''">
            <Money :minor="adjustment.amount_minor" signed /> on
            {{ formatDate(adjustment.date, formatting) }}
          </span>
          <span class="block text-xs text-muted-foreground">{{ about(adjustment) }}</span>
        </span>
        <button
          v-if="!adjustment.undone_at"
          type="button"
          class="rounded-md border px-3 py-1"
          :disabled="busy"
          @click="undo(adjustment.id)"
        >
          Undo
        </button>
      </li>
    </ul>
    <p v-if="failure && !flag" role="alert" class="text-destructive">{{ failure }}</p>
  </section>
</template>
