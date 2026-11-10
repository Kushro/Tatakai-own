package app.tatakai.next.feature.link

import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlin.test.Test

class ProfileNameTest {
    @Test
    fun `every catalog id is understood`() {
        assertTrue(ProfileName.parse("p-none")!!.none)
        assertEquals(ProfileName(upscaleHeight = 1440), ProfileName.parse("p-anime4k-1440"))
        assertEquals(ProfileName(upscaleHeight = 1080, multiplier = 2), ProfileName.parse("p-anime4k-1080-fg2x"))
        assertEquals(ProfileName(multiplier = 3), ProfileName.parse("p-lsfg-1080-x3"))
        assertEquals(ProfileName(autoFps = 120), ProfileName.parse("p-lsfg-1080-auto120"))
        assertEquals(ProfileName(upscaleHeight = 1080, multiplier = 5), ProfileName.parse("p-anime4k-lsfg-1080-x5"))
        assertEquals(ProfileName(upscaleHeight = 1080, autoFps = 120), ProfileName.parse("p-anime4k-lsfg-1080-auto120"))
    }

    @Test
    fun `unknown ids fall back to the server label`() {
        assertNull(ProfileName.parse("p-something-new"))
        assertNull(ProfileName.parse("p-1080"))
        assertNull(ProfileName.parse("p-lsfg-1080"))
    }
}
