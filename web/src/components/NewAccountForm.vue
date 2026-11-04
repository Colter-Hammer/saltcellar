<script setup lang="ts">
// Accounts › + › New account (docs/spec/m2-bank-data.md#accounts): a manual account's name, type
// and role, then what's in it today, which becomes its first balance and its opening balance.
// Cash is the type chosen first, a wallet being the usual manual account *(agent)*.
import { ref } from 'vue'
import type { Account, NewAccount } from '@/api/accounts'
import FormField from '@/components/FormField.vue'
import { ACCOUNT_ROLES, ACCOUNT_TYPES } from '@/lib/accounts'
import { type Formatting, formatMoney, parseMoney } from '@/lib/format'

const props = defineProps<{
  formatting: Formatting
  /** The name's error from the server, if it refused it. */
  nameError?: string | undefined
}>()
const emit = defineEmits<{ save: [account: NewAccount]; cancel: [] }>()

const name = ref('')
const type = ref<Account['type']>('cash')
const role = ref<Account['role']>('budgeted')
const balance = ref('')
const balanceError = ref<string>()

function submit() {
  const minor = parseMoney(balance.value, props.formatting)
  if (minor === null) {
    balanceError.value = `Enter an amount, like ${formatMoney(123456, props.formatting)}.`
    return
  }
  balanceError.value = undefined
  emit('save', { name: name.value, type: type.value, role: role.value, balance_minor: minor })
}
</script>

<template>
  <form
    aria-label="New account"
    class="flex flex-col gap-3 rounded-xl border p-4 lg:max-w-md"
    @submit.prevent="submit"
  >
    <FormField v-model="name" name="new-name" label="Name" :error="nameError" required />
    <div class="flex flex-col gap-1.5">
      <label for="new-type" class="text-sm font-medium">Type</label>
      <select id="new-type" v-model="type" class="rounded-md border px-3 py-2">
        <option v-for="kind in ACCOUNT_TYPES" :key="kind.value" :value="kind.value">
          {{ kind.label }}
        </option>
      </select>
    </div>
    <div class="flex flex-col gap-1.5">
      <label for="new-role" class="text-sm font-medium">Role</label>
      <select id="new-role" v-model="role" class="rounded-md border px-3 py-2">
        <option v-for="each in ACCOUNT_ROLES" :key="each.value" :value="each.value">
          {{ each.label }}
        </option>
      </select>
    </div>
    <FormField
      v-model="balance"
      name="new-balance"
      label="What's in it today?"
      :error="balanceError"
      inputmode="decimal"
      autocomplete="off"
      required
    />
    <div class="flex gap-2">
      <button type="submit" class="rounded-md bg-primary px-3 py-2 text-primary-foreground">
        Create account
      </button>
      <button type="button" class="rounded-md border px-3 py-2" @click="emit('cancel')">
        Cancel
      </button>
    </div>
  </form>
</template>
