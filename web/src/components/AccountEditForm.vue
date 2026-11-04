<script setup lang="ts">
// Edits an account (docs/spec/m2-bank-data.md#accounts): every field but its order, which the
// Accounts screen owns. Changing the role or balances-only on an account that has rows asks first.
import { computed, ref } from 'vue'
import type { Account, AccountChange } from '@/api/accounts'
import FormField from '@/components/FormField.vue'
import { ACCOUNT_ROLES, ACCOUNT_TYPES } from '@/lib/accounts'

const props = defineProps<{
  account: Account
  /** The name's error from the server, if it refused it. */
  nameError?: string | undefined
}>()
const emit = defineEmits<{ save: [change: AccountChange]; cancel: [] }>()

const name = ref(props.account.name)
const institution = ref(props.account.institution ?? '')
const type = ref(props.account.type)
const role = ref(props.account.role)
const balancesOnly = ref(props.account.balances_only)
const inNetWorth = ref(props.account.in_net_worth)
const closed = ref(props.account.closed)
const asking = ref(false)

const countingChanged = computed(
  () => role.value !== props.account.role || balancesOnly.value !== props.account.balances_only,
)

function save() {
  emit('save', {
    name: name.value,
    institution: institution.value,
    type: type.value,
    role: role.value,
    balances_only: balancesOnly.value,
    in_net_worth: inNetWorth.value,
    closed: closed.value,
  })
}

function submit() {
  if (countingChanged.value && props.account.rows > 0) asking.value = true
  else save()
}
</script>

<template>
  <form
    aria-label="Edit account"
    class="flex flex-col gap-3 rounded-xl border p-4 lg:max-w-md"
    @submit.prevent="submit"
  >
    <FormField v-model="name" name="name" label="Name" :error="nameError" required />
    <FormField v-model="institution" name="institution" label="Institution" />
    <div class="flex flex-col gap-1.5">
      <label for="type" class="text-sm font-medium">Type</label>
      <select id="type" v-model="type" class="rounded-md border px-3 py-2">
        <option v-for="kind in ACCOUNT_TYPES" :key="kind.value" :value="kind.value">
          {{ kind.label }}
        </option>
      </select>
    </div>
    <div class="flex flex-col gap-1.5">
      <label for="role" class="text-sm font-medium">Role</label>
      <select id="role" v-model="role" class="rounded-md border px-3 py-2">
        <option v-for="each in ACCOUNT_ROLES" :key="each.value" :value="each.value">
          {{ each.label }}
        </option>
      </select>
    </div>
    <label class="flex items-center gap-2">
      <input v-model="balancesOnly" type="checkbox" /> Balances only
    </label>
    <label class="flex items-center gap-2">
      <input v-model="inNetWorth" type="checkbox" /> In net worth
    </label>
    <label class="flex items-center gap-2">
      <input v-model="closed" type="checkbox" /> Closed
    </label>

    <div
      v-if="asking"
      role="alertdialog"
      aria-labelledby="counting-question"
      class="flex flex-col gap-2 rounded-md border p-3"
    >
      <p id="counting-question">This changes what counts toward budgets.</p>
      <div class="flex gap-2">
        <button
          type="button"
          class="rounded-md bg-primary px-3 py-2 text-primary-foreground"
          @click="save"
        >
          Change it
        </button>
        <button type="button" class="rounded-md border px-3 py-2" @click="asking = false">
          Cancel
        </button>
      </div>
    </div>
    <div v-else class="flex gap-2">
      <button type="submit" class="rounded-md bg-primary px-3 py-2 text-primary-foreground">
        Save
      </button>
      <button type="button" class="rounded-md border px-3 py-2" @click="emit('cancel')">
        Cancel
      </button>
    </div>
  </form>
</template>
