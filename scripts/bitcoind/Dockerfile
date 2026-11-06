FROM bitcoin/bitcoin:29.5

COPY entrypoint.sh /vaca-bitcoind.sh
RUN chmod 755 /vaca-bitcoind.sh

ENTRYPOINT ["/vaca-bitcoind.sh"]
