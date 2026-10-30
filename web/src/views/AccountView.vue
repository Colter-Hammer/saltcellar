<script setup lang="ts">
// An account's detail (docs/spec/m1-ledger-import.md#accounts): the balance shown (its latest
// snapshot) with the app's own beside it, the opening balance and its date, the row count, and its
// transactions. A balances-only account has only its balance.
import { computed } from 'vue'
import { useRoute } from 'vue-router'
import { useAccount } from '@/api/accounts'
import { roleLabel, transactionsLink, typeLabel } from '@/lib/accounts'
import { formatDate, formatMoney, useFormatting } from '@/lib/format'

const route = useRoute()
const query = useAccount(() => Number(route.params.id))
const formatting = useFormatting()

const account = computed(() => query.data.value)
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
  if (balance === null || date === null) return '—'
  return `${money(balance)} on ${formatDate(date, formatting.value)}`
})

function money(minor: number | null): string {
  return minor === null ? '—' : formatMoney(minor, formatting.value)
}
</script>

<template>
  <div class="flex flex-col gap-4 p-4 lg:px-6">
    <RouterLink to="/accounts" class="self-start text-sm text-muted-foreground hover:underline">
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

      <dl
        aria-label="Figures"
        class="grid grid-cols-[auto_1fr] gap-x-6 gap-y-2 rounded-xl border p-4 text-sm lg:max-w-md"
      >
        <dt class="text-muted-foreground">Balance</dt>
        <dd class="text-right tabular-nums">{{ money(account.balance_minor) }}</dd>
        <template v-if="!account.balances_only">
          <!-- The app's own balance: opening plus transactions. Equal in milestone 1; where they
               differ, that's drift, which milestone 2 surfaces. -->
          <dt class="text-muted-foreground">App's balance</dt>
          <dd class="text-right tabular-nums">{{ money(account.app_balance_minor) }}</dd>
          <dt class="text-muted-foreground">Opening balance</dt>
          <dd class="text-right tabular-nums">{{ opening }}</dd>
          <dt class="text-muted-foreground">Transactions</dt>
          <dd class="text-right tabular-nums">
            {{ new Intl.NumberFormat(formatting.locale).format(account.rows) }}
          </dd>
        </template>
      </dl>

      <p v-if="account.balances_only" class="text-sm text-muted-foreground">
        Balances only: it holds balances, not transactions.
      </p>
      <RouterLink v-if="link" :to="link" class="self-start rounded-md border px-3 py-2">
        See transactions
      </RouterLink>
    </section>
  </div>
</template>
