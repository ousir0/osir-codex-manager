import unittest

from awai_connect_relay import RelayError, parse_connect_request


def request(target: str, version: str = "HTTP/1.1") -> bytes:
    return f"CONNECT {target} {version}\r\nHost: {target}\r\n\r\n".encode("ascii")


class ConnectRequestTests(unittest.TestCase):
    def test_allows_only_the_exact_i18n_upstream(self):
        self.assertEqual(
            parse_connect_request(request("ab.chatgpt.com:443")),
            ("ab.chatgpt.com", 443),
        )

    def test_rejects_other_hosts_and_ports(self):
        for target in ("chatgpt.com:443", "ab.chatgpt.com:80", "example.com:443"):
            with self.subTest(target=target):
                with self.assertRaises(RelayError):
                    parse_connect_request(request(target))

    def test_rejects_non_connect_and_non_http11_requests(self):
        with self.assertRaises(RelayError):
            parse_connect_request(b"GET https://ab.chatgpt.com/ HTTP/1.1\r\n\r\n")
        with self.assertRaises(RelayError):
            parse_connect_request(request("ab.chatgpt.com:443", "HTTP/1.0"))


if __name__ == "__main__":
    unittest.main()
