<script setup lang="ts">
// An account's detail (docs/spec/m1-ledger-import.md#accounts): the balance shown (its latest
// snapshot) with the app's own beside it, the opening balance and its date, the row count, and its
// transactions. A balances-only account has only its balance. Edit opens the account's form
// (docs/spec/m2-bank-data.md#accounts). Update balance, on an account with no connection (or a
// balances-only one whose bank link is broken, docs/spec/m2-bank-data.md#errors), records
// what's in it today (docs/spec/m2-bank-data.md#manual-entry). Drift (docs/spec/m2-bank-data.md#balances)
// is measured at each balance snapshot: the figures show the last one's, and a drift that held for
// two snapshots shows its flag, suspect rows and Accept above them (DriftPanel). A balance the
// bank sent with the wrong sign says it was corrected.
import { computed, ref } from 'vue'
import { useRoute } from 'vue-router'
import { useAccount, useAccountEdits, type AccountChange } from '@/api/accounts'
import { ApiError } from '@/api/client'
import AccountEditForm from '@/components/AccountEditForm.vue'
import BalanceForm from '@/components/BalanceForm.vue'
import BankFix from '@/components/BankFix.vue'
import DriftPanel from '@/components/DriftPanel.vue'
import { roleLabel, transactionsLink, typeLabel } from '@/lib/accounts'
import Money from '@/components/Money.vue'
import { formatDate, useFormatting } from '@/lib/format'

const route = useRoute()
const query = useAccount(() => Number(route.params.id))
const formatting = useFormatting()
const edits = useAccountEdits()

const editing = ref(false)
const updating = ref(false)
const nameError = ref<string>()
const problem = ref<string>()

async function save(change: AccountChange) {
  nameError.value = undefined
  problem.value = undefined
  try {
    await edits.change(Number(route.params.id), change)
    editing.value = false
  } catch (error) {
    const name = error instanceof ApiError ? error.fields.name : undefined
    if (name !== undefined) nameError.value = name
    else problem.value = error instanceof Error ? error.message : String(error)
  }
}

async function saveBalance(balanceMinor: number) {
  problem.value = undefined
  try {
    await edits.updateBalance(Number(route.params.id), balanceMinor)
    updating.value = false
  } catch (error) {
    problem.value = error instanceof Error ? error.message : String(error)
  }
}

const account = computed(() => query.data.value)
const syncLabel = computed(() =>
  account.value?.sync?.state === 'stale' ? 'Stale' : 'Needs attention',
)
const link = computed(() => (account.value ? transactionsLink(account.value) : null))
const about = computed(() => {
  const a = account.value
  if (!a) return ''
  return [typeLabel(a.type), roleLabel(a.role), a.institution, a.closed ? 'Closed' : null]
    .filter((part) => part !== null)
    .join(' · ')
})

/** The opening balance and its date, like "€1,500.00 on Dec 31, 2025". */
const opening = computed(() => {
  const balance = account.value?.opening_balance_minor ?? null
  const date = account.value?.opening_date ?? null
  if (balance === null || date === null) return null
  return { balance, date: formatDate(date, formatting.value) }
})
</script>

<template>
  <div class="flex flex-col gap-4 p-4 lg:px-6">
    <RouterLink
      to="/accounts"
      class="inline-flex items-center self-start text-sm text-muted-foreground hover:underline"
    >
      ← All accounts
    </RouterLink>

    <p v-if="query.isError.value" role="alert" class="text-sm text-destructive">
      {{ query.error.value?.message }}
    </p>

    <section v-if="account" aria-labelledby="account-name" class="flex flex-col gap-4">
      <div>
        <h2 id="account-name" class="text-lg font-medium">{{ account.name }}</h2>
        <p class="text-sm text-muted-foreground">{{ about }}</p>
      </div>
      <p v-if="problem" role="alert" class="text-sm text-destructive">{{ problem }}</p>
      <div
        v-if="account.sync && account.sync.state !== 'ok'"
        data-testid="account-sync"
        :data-state="account.sync.state"
        class="flex flex-col items-start gap-2 text-sm"
      >
        <p :class="account.sync.state === 'stale' ? 'text-muted-foreground' : 'text-destructive'">
          <span class="font-medium">{{ syncLabel }}.</span>
          {{ account.sync.message }}
        </p>
        <BankFix v-if="account.sync.fix" />
      </div>
      <DriftPanel :account="account" />
      <AccountEditForm
        v-if="editing"
        :account="account"
        :name-error="nameError"
        @save="save"
        @cancel="((editing = false), (nameError = undefined), (problem = undefined))"
      />
      <BalanceForm
        v-else-if="updating"
        :formatting="formatting"
        @save="saveBalance"
        @cancel="((updating = false), (problem = undefined))"
      />
      <div v-else class="flex flex-wrap gap-2">
        <button
          type="button"
          class="inline-flex items-center rounded-md border px-3 py-2"
          @click="editing = true"
        >
          Edit
        </button>
        <button
          v-if="account.balance_entry === 'manual'"
          type="button"
          class="inline-flex items-center rounded-md border px-3 py-2"
          @click="updating = true"
        >
          Update balance
        </button>
      </div>

      <dl
        aria-label="Figures"
        class="grid grid-cols-[auto_1fr] gap-x-6 gap-y-2 rounded-xl border p-4 lg:max-w-md"
      >
        <dt class="text-muted-foreground">Balance</dt>
        <dd class="text-right">
          <Money v-if="account.balance_minor !== null" :minor="account.balance_minor" />
          <template v-else>—</template>
          <span
            v-if="account.drift.sign_corrected"
            data-testid="sign-corrected"
            class="block text-xs text-muted-foreground"
            title="The bank sent this balance with the wrong sign; the app flipped it."
          >
            Sign corrected
          </span>
        </dd>
        <template v-if="!account.balances_only">
          <!-- The app's own balance: opening plus posted transactions and balance adjustments. -->
          <dt class="text-muted-foreground">App's balance</dt>
          <dd class="text-right">
            <Money v-if="account.app_balance_minor !== null" :minor="account.app_balance_minor" />
            <template v-else>—</template>
          </dd>
          <template
            v-if="account.drift.bank_comparison && account.drift.bank_comparison.drift_minor !== 0"
          >
            <dt class="text-muted-foreground">Off at last balance</dt>
            <dd class="text-right" data-testid="drift">
              <Money :minor="account.drift.bank_comparison.drift_minor" />
              <span class="block text-xs text-muted-foreground">
                {{ formatDate(account.drift.bank_comparison.as_of, formatting) }}
              </span>
            </dd>
          </template>
          <dt class="text-muted-foreground">Opening balance</dt>
          <dd class="text-right">
            <template v-if="opening">
              <Money :minor="opening.balance" /> on {{ opening.date }}
            </template>
            <template v-else>—</template>
          </dd>
          <dt class="text-muted-foreground">Transactions</dt>
          <dd class="text-right tabular-nums">
            {{ new Intl.NumberFormat(formatting.locale).format(account.rows) }}
          </dd>
        </template>
      </dl>

      <p v-if="account.balances_only" class="text-sm text-muted-foreground">
        Balances only: it holds balances, not transactions.
      </p>
      <RouterLink
        v-if="link"
        :to="link"
        class="inline-flex items-center self-start rounded-md border px-3 py-2"
      >
        See transactions
      </RouterLink>
    </section>
  </div>
</template>
