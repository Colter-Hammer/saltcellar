<script setup lang="ts">
// One Actual account in the importer's Accounts step: its figures, and what happens to it. A row
// of the step's table on desktop; stacked on a phone.
import { computed } from 'vue'
import type { ActualAccount, ActualImport } from '@/api/imports'
import { formatMoney, type Formatting } from '@/lib/format'
import { ACCOUNT_ROLES, ACCOUNT_TYPES } from '@/lib/accounts'
import type { AccountChoice } from '@/lib/import'

const props = defineProps<{
  account: ActualAccount
  appAccounts: ActualImport['app_accounts']
  /** Makes the row's ids unique on the page. */
  index: number
  problem: string | undefined
  formatting: Formatting
}>()
const choice = defineModel<AccountChoice>({ required: true })

const id = computed(() => `account-${String(props.index)}`)

function change(changes: Partial<AccountChoice>) {
  choice.value = { ...choice.value, ...changes }
}

function chosen(event: Event): string {
  return event.target instanceof HTMLSelectElement || event.target instanceof HTMLInputElement
    ? event.target.value
    : ''
}
</script>

<template>
  <tr role="row" class="align-top max-lg:block max-lg:border-b max-lg:py-3">
    <th
      scope="row"
      role="rowheader"
      class="py-2 pr-3 text-left font-medium max-lg:block max-lg:p-0"
    >
      {{ account.name }}
      <span v-if="account.closed" class="ml-1 rounded border px-1.5 text-xs text-muted-foreground">
        Closed
      </span>
      <span
        v-if="account.off_budget"
        class="ml-1 rounded border px-1.5 text-xs text-muted-foreground"
      >
        Off budget
      </span>
    </th>
    <td
      role="cell"
      class="py-2 pr-3 tabular-nums max-lg:inline max-lg:p-0 max-lg:text-sm max-lg:text-muted-foreground lg:text-right"
    >
      {{ new Intl.NumberFormat(formatting.locale).format(account.rows) }}
      <span class="lg:hidden">{{ account.rows === 1 ? 'row' : 'rows' }} · </span>
    </td>
    <td
      role="cell"
      class="py-2 pr-3 tabular-nums max-lg:inline max-lg:p-0 max-lg:text-sm max-lg:text-muted-foreground lg:text-right"
    >
      {{ formatMoney(account.balance_minor, formatting) }}
    </td>
    <td role="cell" class="py-2 max-lg:mt-2 max-lg:block max-lg:p-0">
      <label :for="`${id}-action`" class="sr-only">What happens to {{ account.name }}</label>
      <select
        :id="`${id}-action`"
        :value="choice.action"
        class="rounded-md border bg-background px-2 py-1"
        @change="change({ action: chosen($event) as AccountChoice['action'] })"
      >
        <option value="create">Create new</option>
        <option value="same" :disabled="appAccounts.length === 0">Same as…</option>
        <option value="skip">Skip</option>
      </select>

      <div v-if="choice.action === 'create'" class="mt-2 grid gap-2 sm:grid-cols-2">
        <div class="flex flex-col gap-1">
          <label :for="`${id}-name`" class="text-sm font-medium">
            Name<span class="sr-only"> for {{ account.name }}</span>
          </label>
          <input
            :id="`${id}-name`"
            :value="choice.name"
            :aria-invalid="problem ? true : undefined"
            :aria-describedby="problem ? `${id}-problem` : undefined"
            class="rounded-md border px-2 py-1"
            @input="change({ name: chosen($event) })"
          />
        </div>
        <div class="flex flex-col gap-1">
          <label :for="`${id}-type`" class="text-sm font-medium">
            Type<span class="sr-only"> for {{ account.name }}</span>
          </label>
          <select
            :id="`${id}-type`"
            :value="choice.type"
            class="rounded-md border bg-background px-2 py-1"
            @change="change({ type: chosen($event) as AccountChoice['type'] })"
          >
            <option v-for="kind in ACCOUNT_TYPES" :key="kind.value" :value="kind.value">
              {{ kind.label }}
            </option>
          </select>
        </div>
        <div class="flex flex-col gap-1">
          <label :for="`${id}-role`" class="text-sm font-medium">
            Role<span class="sr-only"> for {{ account.name }}</span>
          </label>
          <select
            :id="`${id}-role`"
            :value="choice.role"
            class="rounded-md border bg-background px-2 py-1"
            @change="change({ role: chosen($event) as AccountChoice['role'] })"
          >
            <option v-for="role in ACCOUNT_ROLES" :key="role.value" :value="role.value">
              {{ role.label }}
            </option>
          </select>
        </div>
        <div class="flex items-center gap-2 sm:pt-6">
          <input
            :id="`${id}-balances-only`"
            type="checkbox"
            :checked="choice.balancesOnly"
            @change="change({ balancesOnly: ($event.target as HTMLInputElement).checked })"
          />
          <label :for="`${id}-balances-only`" class="text-sm">
            Balances only<span class="sr-only"> for {{ account.name }}</span>
          </label>
        </div>
        <p class="text-sm text-muted-foreground sm:col-span-2">
          <template v-if="choice.balancesOnly">Keeps its balance, not its rows. </template>
          <template v-if="account.closed">Created closed.</template>
        </p>
      </div>

      <div v-else-if="choice.action === 'same'" class="mt-2 flex flex-col gap-1">
        <label :for="`${id}-same-as`" class="text-sm font-medium">
          Same as<span class="sr-only"> for {{ account.name }}</span>
        </label>
        <select
          :id="`${id}-same-as`"
          :value="choice.sameAs ?? ''"
          :aria-invalid="problem ? true : undefined"
          :aria-describedby="problem ? `${id}-problem` : undefined"
          class="rounded-md border bg-background px-2 py-1"
          @change="change({ sameAs: Number(chosen($event)) })"
        >
          <option value="" disabled>Choose an account</option>
          <option v-for="app in appAccounts" :key="app.id" :value="app.id">{{ app.name }}</option>
        </select>
      </div>

      <p v-else class="mt-2 text-sm text-muted-foreground">None of its rows are imported.</p>

      <p v-if="problem" :id="`${id}-problem`" class="mt-1 text-sm text-destructive">
        {{ problem }}
      </p>
    </td>
  </tr>
</template>
