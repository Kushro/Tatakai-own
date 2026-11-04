// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.ui.state

import androidx.compose.runtime.Composable

// android wave contract (docs/redesign/12 §3.9). Owner: worker b. Name and signature FIXED.

/** true while the default network has INTERNET + VALIDATED (ConnectivityManager callback, lifecycle-aware). Drives the offline banner. Body: worker b. */
@Composable
fun rememberIsOnline(): Boolean = true
