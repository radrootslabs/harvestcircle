<script lang="ts">
  import type { FoodProjection } from '../contracts/food-availability-v1/read.ts';
  let { food, createdAt }: { food: FoodProjection; createdAt: number } =
    $props();
  function timestamp(seconds: number): string {
    const milliseconds = seconds * 1000;
    return Number.isFinite(milliseconds) &&
      Math.abs(milliseconds) <= 8640000000000000
      ? new Date(milliseconds).toISOString()
      : `${seconds} seconds since the Unix epoch`;
  }
</script>

<div class="stack">
  <p>{food.price.currency} {food.price.amount} / {food.price.unit}</p>
  <p>Public pickup area: {food.location}</p>
  {#if food.quantity}<p>
      Advertised quantity: {food.quantity.amount}
      {food.quantity.unit}
    </p>{:else}<p>Advertised quantity not specified</p>{/if}
  <p>Seller's status: {food.status === 'sold' ? 'Sold' : 'Available'}</p>
  <p>Seller updated: {timestamp(createdAt)}</p>
  <p>{food.content}</p>
</div>
