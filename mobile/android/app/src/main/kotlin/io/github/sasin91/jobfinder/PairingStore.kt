package io.github.sasin91.jobfinder

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** Android's own keystore, where the key that locks the token lives; it never leaves it. */
private const val KEYSTORE = "AndroidKeyStore"

/** The key's name in the keystore. */
private const val KEY_NAME = "jobfinder-device-token"

/** AES in GCM mode: the token can't be read or changed without the key. */
private const val TRANSFORMATION = "AES/GCM/NoPadding"

/** Bits of the AES key. */
private const val KEY_BITS = 256

/** Bits of GCM's tag, the most it has. */
private const val TAG_BITS = 128

private const val PREFERENCES = "pairing"
private const val ADDRESS = "address"
private const val TOKEN = "token"
private const val TOKEN_IV = "token_iv"

/** Where this device is paired: the computer's address, and this device's token. */
data class Pairing(val address: String, val token: String)

/**
 * Keeps the pairing on the device. The token is like an API token, so it is kept encrypted with a
 * key in the Android keystore, which can't be taken off the device; the address is kept as is.
 * The preferences file isn't backed up (allowBackup is off).
 */
class PairingStore(context: Context) {
    private val preferences = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)

    /** The pairing kept, or null when this device isn't paired or its token can't be read. */
    fun read(): Pairing? {
        val address = preferences.getString(ADDRESS, null) ?: return null
        val sealed = preferences.getString(TOKEN, null) ?: return null
        val iv = preferences.getString(TOKEN_IV, null) ?: return null
        return runCatching {
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(TAG_BITS, decode(iv)))
            Pairing(address, String(cipher.doFinal(decode(sealed)), Charsets.UTF_8))
        }.getOrNull()
    }

    fun save(pairing: Pairing) {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, key())
        val sealed = cipher.doFinal(pairing.token.toByteArray(Charsets.UTF_8))
        preferences.edit()
            .putString(ADDRESS, pairing.address)
            .putString(TOKEN, encode(sealed))
            .putString(TOKEN_IV, encode(cipher.iv))
            .apply()
    }

    fun forget() {
        preferences.edit().clear().apply()
    }

    private fun key(): SecretKey {
        val keyStore = KeyStore.getInstance(KEYSTORE).apply { load(null) }
        (keyStore.getKey(KEY_NAME, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
        generator.init(
            KeyGenParameterSpec.Builder(KEY_NAME, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(KEY_BITS)
                .build(),
        )
        return generator.generateKey()
    }

    private fun encode(bytes: ByteArray) = Base64.encodeToString(bytes, Base64.NO_WRAP)

    private fun decode(text: String) = Base64.decode(text, Base64.NO_WRAP)
}
