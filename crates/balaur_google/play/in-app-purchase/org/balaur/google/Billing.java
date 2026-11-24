package org.balaur.google;

import android.app.Activity;
import android.content.Intent;

import com.android.billingclient.api.AcknowledgePurchaseParams;
import com.android.billingclient.api.BillingClient;
import com.android.billingclient.api.BillingClientStateListener;
import com.android.billingclient.api.BillingFlowParams;
import com.android.billingclient.api.BillingResult;
import com.android.billingclient.api.ConsumeParams;
import com.android.billingclient.api.PendingPurchasesParams;
import com.android.billingclient.api.ProductDetails;
import com.android.billingclient.api.Purchase;
import com.android.billingclient.api.QueryProductDetailsParams;
import com.android.billingclient.api.QueryPurchasesParams;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

/**
 * Play Billing behind {@code platform.products}, {@code purchase},
 * {@code finish_purchase}, {@code restore_purchases} and
 * {@code entitlements}, in the shapes StoreKit's shim answers with.
 *
 * {@code balaur_google} calls {@link #start} from the game's thread with a
 * call's name and its arguments as JSON. Everything runs on the UI thread,
 * where Billing answers. A purchase that lands with no flow open, one that
 * was pending, arrives under request 0 as {@code transaction}.
 */
public final class Billing {
    private static final String KIND = "kind";
    private static final String ERROR = "error";
    private static final String ID = "id";
    private static final String PRODUCT = "product";
    private static final double MICROS = 1_000_000.0;
    // What Billing binds to, which its manifest's <queries> lets this see.
    private static final String BIND = "com.android.vending.billing.InAppBillingService.BIND";
    private static final String PLAY_STORE = "com.android.vending";

    private static BillingClient client;
    /** The request the open purchase flow answers, 0 when none is open. */
    private static long flow;
    private static String flowProduct = "";
    /** What a products answer told us, which a purchase flow is launched with. */
    private static final Map<String, ProductDetails> KNOWN = new HashMap<>();
    /** Calls waiting for the connection, run once it is up, and their requests. */
    private static final List<Runnable> WAITING = new ArrayList<>();
    private static final List<Long> WAITING_REQUESTS = new ArrayList<>();

    private Billing() {}

    /** A request's answer, as JSON. Registered by balaur_google. */
    static native void answer(long request, String json);

    /** Whether the Play Store is here to bill through: its service answers the bind Billing makes. */
    public static boolean available(Activity game) {
        Intent bind = new Intent(BIND).setPackage(PLAY_STORE);
        return !game.getPackageManager().queryIntentServices(bind, 0).isEmpty();
    }

    /** Start {@code call} for {@code request}, with its arguments as JSON. */
    public static void start(final Activity game, final long request, final String call, final String args) {
        game.runOnUiThread(() -> {
            try {
                JSONObject parsed = new JSONObject(args);
                connected(game, request, () -> {
                    try {
                        dispatch(game, request, call, parsed);
                    } catch (JSONException | RuntimeException e) {
                        answer(request, failed(call + ": " + e).toString());
                    }
                });
            } catch (JSONException e) {
                answer(request, failed(call + ": " + e).toString());
            }
        });
    }

    private static void dispatch(Activity game, long request, String call, JSONObject args) throws JSONException {
        switch (call) {
            case "products":
                products(request, args.getJSONArray("ids"), false, game);
                break;
            case "purchase":
                purchase(game, request, args.getString(PRODUCT));
                break;
            case "entitlements":
                owned(request, "entitlements");
                break;
            case "restore_purchases":
                owned(request, "restored");
                break;
            case "finish_purchase":
                finish(request, args.getString("transaction"), args.getBoolean("consume"));
                break;
            default:
                JSONObject out = kind("unsupported");
                out.put("call", call);
                answer(request, out.toString());
        }
    }

    /** Run {@code then} once Billing is connected; answer an error if it cannot be. */
    private static void connected(Activity game, long request, Runnable then) {
        if (client == null) {
            client = BillingClient.newBuilder(game.getApplicationContext())
                    .setListener(Billing::updated)
                    .enablePendingPurchases(PendingPurchasesParams.newBuilder().enableOneTimeProducts().build())
                    .enableAutoServiceReconnection()
                    .build();
        }
        if (client.isReady()) {
            then.run();
            return;
        }
        WAITING.add(then);
        WAITING_REQUESTS.add(request);
        if (WAITING.size() > 1) {
            return;
        }
        client.startConnection(new BillingClientStateListener() {
            @Override
            public void onBillingSetupFinished(BillingResult result) {
                List<Runnable> ready = new ArrayList<>(WAITING);
                List<Long> asked = new ArrayList<>(WAITING_REQUESTS);
                WAITING.clear();
                WAITING_REQUESTS.clear();
                if (result.getResponseCode() == BillingClient.BillingResponseCode.OK) {
                    for (Runnable call : ready) {
                        call.run();
                    }
                    return;
                }
                for (long waiter : asked) {
                    answer(waiter, failed("Play Billing: " + result.getDebugMessage()).toString());
                }
            }

            @Override
            public void onBillingServiceDisconnected() {
                // enableAutoServiceReconnection reconnects on the next call.
            }
        });
    }

    /** Both product types asked for each id, since an id says nothing of its type. */
    private static void products(long request, JSONArray ids, boolean thenBuy, Activity game) throws JSONException {
        List<QueryProductDetailsParams.Product> wanted = new ArrayList<>();
        for (int i = 0; i < ids.length(); i++) {
            for (String type : new String[] {BillingClient.ProductType.INAPP, BillingClient.ProductType.SUBS}) {
                wanted.add(QueryProductDetailsParams.Product.newBuilder()
                        .setProductId(ids.getString(i)).setProductType(type).build());
            }
        }
        if (wanted.isEmpty()) {
            JSONObject out = kind("products");
            out.put("products", new JSONArray());
            answer(request, out.toString());
            return;
        }
        client.queryProductDetailsAsync(
                QueryProductDetailsParams.newBuilder().setProductList(wanted).build(),
                (result, details) -> game.runOnUiThread(() -> {
                    if (result.getResponseCode() != BillingClient.BillingResponseCode.OK) {
                        answer(request, failed("Play Billing: " + result.getDebugMessage()).toString());
                        return;
                    }
                    JSONArray rows = new JSONArray();
                    for (ProductDetails product : details.getProductDetailsList()) {
                        KNOWN.put(product.getProductId(), product);
                        rows.put(describe(product));
                    }
                    if (thenBuy) {
                        launch(game, request, flowProduct);
                        return;
                    }
                    JSONObject out = kind("products");
                    try {
                        out.put("products", rows);
                    } catch (JSONException e) {
                        // A string key and an array never fail.
                    }
                    answer(request, out.toString());
                }));
    }

    private static JSONObject describe(ProductDetails product) {
        JSONObject row = new JSONObject();
        try {
            row.put(ID, product.getProductId());
            row.put("title", product.getName());
            row.put("description", product.getDescription());
            boolean subscription = BillingClient.ProductType.SUBS.equals(product.getProductType());
            row.put("subscription", subscription);
            ProductDetails.OneTimePurchaseOfferDetails once = product.getOneTimePurchaseOfferDetails();
            if (once != null) {
                row.put("price", once.getPriceAmountMicros() / MICROS);
                row.put("display_price", once.getFormattedPrice());
                row.put("currency", once.getPriceCurrencyCode());
            }
            List<ProductDetails.SubscriptionOfferDetails> offers = product.getSubscriptionOfferDetails();
            if (offers != null && !offers.isEmpty()) {
                List<ProductDetails.PricingPhase> phases = offers.get(0).getPricingPhases().getPricingPhaseList();
                ProductDetails.PricingPhase phase = phases.get(phases.size() - 1);
                row.put("price", phase.getPriceAmountMicros() / MICROS);
                row.put("display_price", phase.getFormattedPrice());
                row.put("currency", phase.getPriceCurrencyCode());
            }
        } catch (JSONException e) {
            // Keys and plain values never fail.
        }
        return row;
    }

    private static void purchase(Activity game, long request, String product) throws JSONException {
        if (flow != 0) {
            answer(request, failed("a purchase is already open").toString());
            return;
        }
        flow = request;
        flowProduct = product;
        if (KNOWN.containsKey(product)) {
            launch(game, request, product);
        } else {
            products(request, new JSONArray().put(product), true, game);
        }
    }

    private static void launch(Activity game, long request, String product) {
        ProductDetails details = KNOWN.get(product);
        if (details == null) {
            flow = 0;
            answer(request, failed("Play has no product " + product).toString());
            return;
        }
        BillingFlowParams.ProductDetailsParams.Builder item =
                BillingFlowParams.ProductDetailsParams.newBuilder().setProductDetails(details);
        List<ProductDetails.SubscriptionOfferDetails> offers = details.getSubscriptionOfferDetails();
        if (offers != null && !offers.isEmpty()) {
            item.setOfferToken(offers.get(0).getOfferToken());
        }
        List<BillingFlowParams.ProductDetailsParams> items = new ArrayList<>();
        items.add(item.build());
        BillingResult launched = client.launchBillingFlow(game,
                BillingFlowParams.newBuilder().setProductDetailsParamsList(items).build());
        if (launched.getResponseCode() != BillingClient.BillingResponseCode.OK) {
            flow = 0;
            answer(request, failed("Play Billing: " + launched.getDebugMessage()).toString());
        }
    }

    /** What the purchase flow, or Play by itself, says landed. */
    private static void updated(BillingResult result, List<Purchase> purchases) {
        long request = flow;
        String product = flowProduct;
        flow = 0;
        int code = result.getResponseCode();
        if (code == BillingClient.BillingResponseCode.USER_CANCELED) {
            answer(request, withProduct("cancelled", product).toString());
            return;
        }
        if (code != BillingClient.BillingResponseCode.OK || purchases == null) {
            if (request != 0) {
                answer(request, failed("Play Billing: " + result.getDebugMessage()).toString());
            }
            return;
        }
        boolean answered = false;
        for (Purchase purchase : purchases) {
            boolean asked = request != 0 && !answered && purchase.getProducts().contains(product);
            JSONObject out;
            if (purchase.getPurchaseState() == Purchase.PurchaseState.PENDING) {
                out = withProduct("pending", purchase.getProducts().isEmpty() ? product : purchase.getProducts().get(0));
            } else {
                out = transaction(purchase, asked ? "purchased" : "transaction");
            }
            answer(asked ? request : 0, out.toString());
            answered |= asked;
        }
        if (request != 0 && !answered) {
            answer(request, failed("Play Billing answered no purchase of " + product).toString());
        }
    }

    /** The purchases Play holds for this player, both types. */
    private static void owned(long request, String kind) {
        final JSONArray items = new JSONArray();
        client.queryPurchasesAsync(
                QueryPurchasesParams.newBuilder().setProductType(BillingClient.ProductType.INAPP).build(),
                (once, inapp) -> {
                    collect(inapp, items);
                    client.queryPurchasesAsync(
                            QueryPurchasesParams.newBuilder().setProductType(BillingClient.ProductType.SUBS).build(),
                            (again, subs) -> {
                                collect(subs, items);
                                if (once.getResponseCode() != BillingClient.BillingResponseCode.OK) {
                                    answer(request, failed("Play Billing: " + once.getDebugMessage()).toString());
                                    return;
                                }
                                JSONObject out = kind(kind);
                                try {
                                    if (!"restored".equals(kind)) {
                                        out.put("items", items);
                                    }
                                } catch (JSONException e) {
                                    // A string key and an array never fail.
                                }
                                answer(request, out.toString());
                            });
                });
    }

    private static void collect(List<Purchase> purchases, JSONArray into) {
        for (Purchase purchase : purchases) {
            if (purchase.getPurchaseState() == Purchase.PurchaseState.PURCHASED) {
                into.put(transaction(purchase, null));
            }
        }
    }

    /** Acknowledge a purchase, or consume it so it can be bought again. */
    private static void finish(long request, String token, boolean consume) {
        if (consume) {
            client.consumeAsync(ConsumeParams.newBuilder().setPurchaseToken(token).build(),
                    (result, consumed) -> finished(request, token, result));
        } else {
            client.acknowledgePurchase(AcknowledgePurchaseParams.newBuilder().setPurchaseToken(token).build(),
                    result -> finished(request, token, result));
        }
    }

    private static void finished(long request, String token, BillingResult result) {
        if (result.getResponseCode() != BillingClient.BillingResponseCode.OK) {
            answer(request, failed("Play Billing: " + result.getDebugMessage()).toString());
            return;
        }
        answer(request, withField("finished", ID, token).toString());
    }

    /** A purchase as a {@code platform::Transaction}: the token is its id and its proof. */
    private static JSONObject transaction(Purchase purchase, String kind) {
        JSONObject out = kind == null ? new JSONObject() : kind(kind);
        try {
            out.put(ID, purchase.getPurchaseToken());
            out.put(PRODUCT, purchase.getProducts().isEmpty() ? "" : purchase.getProducts().get(0));
            out.put("purchased_at", purchase.getPurchaseTime() / 1000.0);
            out.put("quantity", purchase.getQuantity());
            out.put("revoked", false);
            out.put("token", purchase.getPurchaseToken());
            out.put("verified", false);
        } catch (JSONException e) {
            // Keys and plain values never fail.
        }
        return out;
    }

    private static JSONObject withProduct(String kind, String product) {
        return withField(kind, PRODUCT, product);
    }

    private static JSONObject withField(String kind, String key, String value) {
        JSONObject out = kind(kind);
        try {
            out.put(key, value);
        } catch (JSONException e) {
            // A string key and value never fail.
        }
        return out;
    }

    private static JSONObject kind(String kind) {
        JSONObject out = new JSONObject();
        try {
            out.put(KIND, kind);
        } catch (JSONException e) {
            // A string key and value never fail.
        }
        return out;
    }

    private static JSONObject failed(String message) {
        return withField(ERROR, ERROR, message == null ? "" : message);
    }
}
