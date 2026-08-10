package org.harvestcircle.buildlogic.contracts

import java.security.MessageDigest

internal fun String.sha256(): String =
    MessageDigest
        .getInstance("SHA-256")
        .digest(toByteArray(Charsets.UTF_8))
        .joinToString("") { byte -> "%02x".format(byte) }

internal fun String.isCanonicalHex(width: Int): Boolean =
    length == width && all { character -> character in '0'..'9' || character in 'a'..'f' }
