<script setup lang="ts">
// Accounts (docs/spec/m1-ledger-import.md#accounts), read-only: the Actual importer creates them.
// Phones list them grouped Budgeted / Tracking / Closed; desktops show one table in the same order,
// closed ones marked so *(agent)*. Each opens its detail. With none, the empty state.
import { computed } from 'vue'
import { useAccounts } from '@/api/accounts'
import { groupAccounts, roleLabel, typeLabel } from '@/lib/accounts'
import { formatDate, formatMoney, useFormatting } from '@/lib/format'

const accounts = useAccounts()
const formatting = useFormatting()

const list = computed(() => accounts.data.value?.accounts ?? [])
const groups = computed(() => groupAccounts(list.value))
/** The table's rows: the groups' order, so closed ones come last. */
const rows = computed(() => groups.value.flatMap((group) => group.accounts))
const empty = computed(() => accounts.isSuccess.value && list.value.length === 0)

function balance(minor: number | null): string {
  return minor === null ? '—' : formatMoney(minor, formatting.value)
}
</script>

<template>
  <div class="flex flex-col gap-4 p-4 lg:px-6">
    <p v-if="accounts.isError.value" role="alert" class="text-sm text-destructive">
      {{ accounts.error.value?.message }}
    </p>

    <template v-if="empty">
      <p class="text-sm text-muted-foreground">No accounts yet.</p>
      <div class="flex flex-wrap gap-2">
        <RouterLink to="/import" class="rounded-md bg-primary px-3 py-2 text-primary-foreground">
          Import from Actual
        </RouterLink>
      </div>
    </template>

    <template v-else-if="list.length > 0">
      <!-- Phones: the groups. -->
      <section
        v-for="group in groups"
        :key="group.name"
        :aria-labelledby="`group-${group.name}`"
        class="flex flex-col gap-2 lg:hidden"
      >
        <h2 :id="`group-${group.name}`" class="text-sm font-medium text-muted-foreground">
          {{ group.name }}
        </h2>
        <ul class="divide-y rounded-xl border">
          <li v-for="account in group.accounts" :key="account.id">
            <RouterLink
              :to="`/accounts/${String(account.id)}`"
              class="flex items-center gap-3 px-4 py-3"
            >
              <span class="flex min-w-0 flex-1 flex-col">
                <span class="truncate">{{ account.name }}</span>
                <span class="text-xs text-muted-foreground">{{ typeLabel(account.type) }}</span>
              </span>
              <span class="tabular-nums">{{ balance(account.balance_minor) }}</span>
            </RouterLink>
          </li>
        </ul>
      </section>

      <!-- Desktops: the table. -->
      <table aria-label="Accounts" class="w-full text-sm max-lg:hidden">
        <thead class="text-left text-muted-foreground">
          <tr class="border-b">
            <th scope="col" class="py-2 pr-3 font-medium">Name</th>
            <th scope="col" class="py-2 pr-3 font-medium">Institution</th>
            <th scope="col" class="py-2 pr-3 font-medium">Type</th>
            <th scope="col" class="py-2 pr-3 font-medium">Role</th>
            <th scope="col" class="py-2 pr-3 text-right font-medium">Balance</th>
            <th scope="col" class="py-2 text-right font-medium">Last transaction</th>
          </tr>
        </thead>
        <tbody class="divide-y">
          <tr v-for="account in rows" :key="account.id">
            <th scope="row" class="py-2 pr-3 text-left font-normal">
              <RouterLink :to="`/accounts/${String(account.id)}`" class="hover:underline">
                {{ account.name }}
              </RouterLink>
              <span v-if="account.closed" class="ml-2 text-xs text-muted-foreground">Closed</span>
            </th>
            <td class="py-2 pr-3">{{ account.institution ?? '—' }}</td>
            <td class="py-2 pr-3">{{ typeLabel(account.type) }}</td>
            <td class="py-2 pr-3">{{ roleLabel(account.role) }}</td>
            <td class="py-2 pr-3 text-right tabular-nums">
              {{ balance(account.balance_minor) }}
            </td>
            <td class="py-2 text-right tabular-nums">
              {{
                account.last_transaction_date === null
                  ? '—'
                  : formatDate(account.last_transaction_date, formatting)
              }}
            </td>
          </tr>
        </tbody>
      </table>
    </template>
  </div>
</template>
