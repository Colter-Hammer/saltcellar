<script setup lang="ts">
// Tracking one account the bank connection reports (docs/spec/m2-bank-data.md#accounts): its
// role (the button pressed chooses it first), whether only its balance is kept (suggested from
// its name), and "This is the same as…", an existing account to move onto it, or a new one.
import { ref } from 'vue'
import type { Account } from '@/api/accounts'
import type { FeedAccount, TrackFeedAccount } from '@/api/connectionAccounts'
import { ACCOUNT_ROLES } from '@/lib/accounts'
import { sameAsChoices } from '@/lib/connectionAccounts'

const props = defineProps<{
  account: FeedAccount
  role: Account['role']
  /** Every account, for "This is the same as…". */
  accounts: readonly Account[]
}>()
const emit = defineEmits<{ save: [track: TrackFeedAccount]; cancel: [] }>()

const id = props.account.external_account_id
const role = ref(props.role)
const balancesOnly = ref(props.account.balances_only_suggested)
/** An account id, or null for a new account. */
const sameAs = ref<number | null>(props.account.same_as_account_id)
const choices = sameAsChoices(props.accounts, props.account.same_as_account_id)

function submit() {
  emit('save', {
    role: role.value,
    balances_only: balancesOnly.value,
    same_as_account_id: sameAs.value,
  })
}
</script>

<template>
  <form
    :aria-label="`Track ${account.name}`"
    class="flex flex-col gap-3 rounded-lg border p-3"
    @submit.prevent="submit"
  >
    <div class="flex flex-col gap-1.5">
      <label :for="`track-role-${id}`" class="text-sm font-medium">Role</label>
      <select :id="`track-role-${id}`" v-model="role" class="rounded-md border px-3 py-2">
        <option v-for="each in ACCOUNT_ROLES" :key="each.value" :value="each.value">
          {{ each.label }}
        </option>
      </select>
    </div>
    <label class="flex items-center gap-2 text-sm">
      <input v-model="balancesOnly" type="checkbox" />
      Balances only (investments, loans): keep its balance, not its transactions
    </label>
    <div class="flex flex-col gap-1.5">
      <label :for="`track-same-${id}`" class="text-sm font-medium">This is the same as…</label>
      <select :id="`track-same-${id}`" v-model="sameAs" class="rounded-md border px-3 py-2">
        <option :value="null">Nothing yet: a new account</option>
        <option v-for="each in choices" :key="each.id" :value="each.id">{{ each.name }}</option>
      </select>
    </div>
    <div class="flex gap-2">
      <button type="submit" class="rounded-md bg-primary px-3 py-2 text-primary-foreground">
        Track
      </button>
      <button type="button" class="rounded-md border px-3 py-2" @click="emit('cancel')">
        Cancel
      </button>
    </div>
  </form>
</template>
