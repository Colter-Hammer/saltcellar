<script setup lang="ts">
// Shown on every screen while the bank connection is paused (docs/spec/m2-bank-data.md#sync-runs):
// nothing syncs until a new setup token is pasted in Settings.
import { computed } from 'vue'
import { useBankConnection } from '@/api/bank'
import { pausedBanner } from '@/lib/sync'

const bank = useBankConnection()
const banner = computed(() => {
  const connection = bank.data.value?.connection
  return connection?.paused ? pausedBanner(connection.status) : null
})
</script>

<template>
  <div
    v-if="banner"
    role="status"
    data-testid="paused-banner"
    class="border-b border-destructive/30 bg-destructive/10 px-4 py-2 text-sm text-destructive lg:px-6"
  >
    {{ banner.lead
    }}<RouterLink to="/settings/bank" class="font-medium underline">{{ banner.link }}</RouterLink
    >{{ banner.tail }}
  </div>
</template>
