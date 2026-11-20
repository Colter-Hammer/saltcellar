<script setup lang="ts">
// The one status spot (docs/spec/m2-bank-data.md#sync-health-in-the-ui): a pill in the phone
// header, a button at the foot of the sidebar. A hollow dot when all is good, a filled one when
// something needs the household. It opens the sync sheet.
import type { Spot } from '@/lib/syncHealth'
import { cn } from '@/lib/utils'

defineProps<{ spot: Spot; variant: 'pill' | 'button' }>()
</script>

<template>
  <button
    type="button"
    data-testid="sync-spot"
    :data-variant="variant"
    :data-problem="spot.filled"
    :class="
      cn(
        'inline-flex items-center gap-2 text-sm',
        variant === 'pill' ? 'rounded-full border px-3 py-1 lg:hidden' : 'rounded-md px-2 py-1.5',
        spot.filled && 'font-medium text-destructive',
      )
    "
  >
    <span
      aria-hidden="true"
      :class="cn('size-2 shrink-0 rounded-full border border-current', spot.filled && 'bg-current')"
    />
    {{ variant === 'pill' ? spot.short : spot.long }}
  </button>
</template>
