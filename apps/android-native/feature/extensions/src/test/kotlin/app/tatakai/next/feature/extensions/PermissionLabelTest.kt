// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.extensions

import kotlin.test.Test
import kotlin.test.assertEquals

class PermissionLabelTest {
    @Test
    fun permissionIdsBecomeSitesAndWords() {
        // BUG-075: "network:domain:az-animex.com" chips -> "az-animex.com"; the rest in words, as on the desktop.
        assertEquals(null to "az-animex.com", permissionLabel("network:domain:az-animex.com"))
        assertEquals(PermissionLabel.NETWORK to null, permissionLabel("network:any"))
        assertEquals(PermissionLabel.STORAGE to null, permissionLabel("storage:local"))
        assertEquals(PermissionLabel.TORRENT to null, permissionLabel("torrent:session"))
        assertEquals(null to null, permissionLabel("parse:html"))
    }
}
